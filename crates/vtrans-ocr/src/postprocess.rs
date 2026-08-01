//! Post-processing for OCR: probability-map boxes, reading-order sorting,
//! CTC decoding, confidence filtering, and text merging.

// Component labeling and box math work in pixel coordinates; conversions
// are bounded by the image size.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

use ndarray::Array2;

use vtrans_core::error::OcrError;
use vtrans_core::types::{OcrLine, OcrOptions};

use crate::geometry::{dilation_distance, min_area_rect, offset_polygon, polygon_center, Point};

/// Detection post-processing parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectionParams {
    /// Binarization threshold for the probability map.
    pub threshold: f32,
    /// Unclip ratio used to expand detected boxes.
    pub unclip_ratio: f32,
    /// Minimum component area in resized-image pixels.
    pub min_box_area: f32,
}

/// A detected text box in original image coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedBox {
    /// Four polygon corners ordered clockwise from the top-left.
    pub polygon: [Point; 4],
    /// Mean probability of the connected component.
    pub score: f32,
    /// Box width in original image pixels.
    pub width: f32,
    /// Box height in original image pixels.
    pub height: f32,
}

/// A recognized text line before final confidence filtering.
#[derive(Debug, Clone, PartialEq)]
pub struct RecognizedLine {
    /// Decoded text.
    pub text: String,
    /// Mean recognition confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

/// Convert a detection probability map into text boxes.
///
/// The map is thresholded, connected components are labeled, each component
/// is replaced by its minimum-area rectangle, the rectangle is expanded with
/// the PP-OCR unclip step, and coordinates are scaled back to the original
/// image space.
///
/// # Example
///
/// ```
/// use ndarray::Array2;
/// use vtrans_ocr::postprocess::{boxes_from_map, DetectionParams};
///
/// let mut probability = Array2::<f32>::zeros((8, 8));
/// for y in 2..6 {
///     for x in 2..6 {
///         probability[[y, x]] = 0.9;
///     }
/// }
/// let params = DetectionParams {
///     threshold: 0.5,
///     unclip_ratio: 0.0,
///     min_box_area: 3.0,
/// };
/// let boxes = boxes_from_map(&probability, params, 1.0, 1.0, 8, 8);
/// assert_eq!(boxes.len(), 1);
/// assert!((boxes[0].width - 4.0).abs() < 1.5);
/// ```
#[must_use]
pub fn boxes_from_map(
    probability: &Array2<f32>,
    params: DetectionParams,
    ratio_x: f32,
    ratio_y: f32,
    original_width: u32,
    original_height: u32,
) -> Vec<DetectedBox> {
    let (height, width) = probability.dim();
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let mut visited = vec![false; width * height];
    let mut stack = Vec::new();
    let mut boxes = Vec::new();

    for start_y in 0..height {
        for start_x in 0..width {
            let start = start_y * width + start_x;
            if visited[start] || probability[[start_y, start_x]] < params.threshold {
                continue;
            }

            let mut points = Vec::new();
            let mut score_sum = 0.0_f32;
            stack.push((start_x, start_y));
            visited[start] = true;
            while let Some((x, y)) = stack.pop() {
                points.push([x as f32 + 0.5, y as f32 + 0.5]);
                score_sum += probability[[y, x]];
                for dy in -1_i32..=1 {
                    for dx in -1_i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                            continue;
                        }
                        let index = ny as usize * width + nx as usize;
                        if !visited[index]
                            && probability[[ny as usize, nx as usize]] >= params.threshold
                        {
                            visited[index] = true;
                            stack.push((nx as usize, ny as usize));
                        }
                    }
                }
            }

            if (points.len() as f32) < params.min_box_area {
                continue;
            }
            let score = score_sum / points.len() as f32;
            let mut polygon = min_area_rect(&points);
            let distance = dilation_distance(&polygon, params.unclip_ratio);
            if distance > 0.0 {
                polygon = offset_polygon(polygon, distance);
            }
            let polygon = clip_polygon(&polygon, width, height);
            let polygon =
                scale_polygon(&polygon, ratio_x, ratio_y, original_width, original_height);
            let (box_width, box_height) = box_dimensions(&polygon);
            boxes.push(DetectedBox {
                polygon,
                score,
                width: box_width,
                height: box_height,
            });
        }
    }
    boxes
}

/// Sort detected boxes into reading order.
///
/// Horizontal text is sorted top-to-bottom and then left-to-right. Vertical
/// text is sorted right-to-left by column and then top-to-bottom within each
/// column. The orientation with the majority of boxes wins.
///
/// # Example
///
/// ```
/// use vtrans_core::types::OcrOptions;
/// use vtrans_ocr::postprocess::{sort_boxes, DetectedBox};
///
/// let make = |x: f32, y: f32| DetectedBox {
///     polygon: [[x, y], [x + 30.0, y], [x + 30.0, y + 10.0], [x, y + 10.0]],
///     score: 1.0,
///     width: 30.0,
///     height: 10.0,
/// };
/// let box_a = make(10.0, 0.0);
/// let box_b = make(0.0, 0.0);
/// let sorted = sort_boxes(vec![box_a, box_b], &OcrOptions::default());
/// assert_eq!(sorted[0].polygon[0], [0.0, 0.0]);
/// ```
#[must_use]
pub fn sort_boxes(boxes: Vec<DetectedBox>, options: &OcrOptions) -> Vec<DetectedBox> {
    let vertical_count = boxes
        .iter()
        .filter(|b| options.detect_vertical && b.height > b.width * 1.5)
        .count();
    let mut boxes = boxes;
    if vertical_count * 2 > boxes.len() {
        sort_vertical(&mut boxes);
    } else {
        sort_horizontal(&mut boxes);
    }
    boxes
}

/// Sort boxes by row (top to bottom) then by column (left to right).
fn sort_horizontal(boxes: &mut Vec<DetectedBox>) {
    boxes.sort_by(|a, b| {
        center_y(a)
            .partial_cmp(&center_y(b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                center_x(a)
                    .partial_cmp(&center_x(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut anchors: Vec<f32> = Vec::new();
    for (index, box_) in boxes.iter().enumerate() {
        let cy = center_y(box_);
        let tolerance = (box_.height * 0.35).max(4.0);
        if let Some(row) = anchors
            .iter()
            .position(|anchor| (cy - anchor).abs() <= tolerance)
        {
            rows[row].push(index);
        } else {
            anchors.push(cy);
            rows.push(vec![index]);
        }
    }

    let mut order = Vec::with_capacity(boxes.len());
    for mut row in rows {
        row.sort_by(|&a, &b| {
            center_x(&boxes[a])
                .partial_cmp(&center_x(&boxes[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        order.extend(row);
    }
    *boxes = order
        .into_iter()
        .map(|index| boxes[index].clone())
        .collect();
}

/// Sort boxes by column (right to left) then by row (top to bottom).
fn sort_vertical(boxes: &mut Vec<DetectedBox>) {
    boxes.sort_by(|a, b| {
        center_x(b)
            .partial_cmp(&center_x(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                center_y(a)
                    .partial_cmp(&center_y(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut columns: Vec<Vec<usize>> = Vec::new();
    let mut anchors: Vec<f32> = Vec::new();
    for (index, box_) in boxes.iter().enumerate() {
        let cx = center_x(box_);
        let tolerance = (box_.width * 0.35).max(4.0);
        if let Some(column) = anchors
            .iter()
            .position(|anchor| (cx - anchor).abs() <= tolerance)
        {
            columns[column].push(index);
        } else {
            anchors.push(cx);
            columns.push(vec![index]);
        }
    }

    let mut order = Vec::with_capacity(boxes.len());
    for mut column in columns {
        column.sort_by(|&a, &b| {
            center_y(&boxes[a])
                .partial_cmp(&center_y(&boxes[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        order.extend(column);
    }
    *boxes = order
        .into_iter()
        .map(|index| boxes[index].clone())
        .collect();
}

/// Drop recognized lines whose confidence is below the threshold.
///
/// # Example
///
/// ```
/// use vtrans_ocr::postprocess::{filter_by_confidence, RecognizedLine};
///
/// let lines = vec![
///     RecognizedLine { text: "keep".to_string(), confidence: 0.9 },
///     RecognizedLine { text: "drop".to_string(), confidence: 0.2 },
/// ];
/// let kept = filter_by_confidence(lines, 0.5);
/// assert_eq!(kept.len(), 1);
/// assert_eq!(kept[0].text, "keep");
/// ```
#[must_use]
pub fn filter_by_confidence(
    lines: Vec<RecognizedLine>,
    min_confidence: f32,
) -> Vec<RecognizedLine> {
    lines
        .into_iter()
        .filter(|line| line.confidence >= min_confidence)
        .collect()
}

/// Merge OCR lines into a single string, ordered by `reading_order`.
///
/// Lines are joined with `\n`, preserving paragraph boundaries.
///
/// # Example
///
/// ```
/// use vtrans_core::types::OcrLine;
/// use vtrans_ocr::postprocess::merge_lines;
///
/// let lines = vec![
///     OcrLine::new("second", 1.0, [[0.0; 2]; 4], 1),
///     OcrLine::new("first", 1.0, [[0.0; 2]; 4], 0),
/// ];
/// assert_eq!(merge_lines(&lines), "first\nsecond");
/// ```
#[must_use]
pub fn merge_lines(lines: &[OcrLine]) -> String {
    let mut sorted = lines.to_vec();
    sorted.sort_by_key(|line| line.reading_order);
    sorted
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Greedy CTC decode a logits sequence.
///
/// Index `0` is treated as the CTC blank. Consecutive repeated labels are
/// collapsed and blanks are removed. The confidence is the mean probability
/// of the emitted non-blank labels.
///
/// # Errors
///
/// Returns [`OcrError::Postprocess`] if the dictionary is empty or the logits
/// length does not match `width * dict.len()`.
///
/// # Example
///
/// ```
/// use vtrans_ocr::postprocess::ctc_greedy_decode;
///
/// let dict = ["", "a", "b"].map(String::from).to_vec();
/// // t0: a, t1: a (repeat), t2: blank, t3: b, t4: blank
/// let logits = [
///     0.1, 0.9, 0.0,
///     0.1, 0.9, 0.0,
///     0.9, 0.1, 0.0,
///     0.1, 0.0, 0.9,
///     0.9, 0.1, 0.0,
/// ];
/// let (text, confidence) = ctc_greedy_decode(&logits, 5, &dict).unwrap();
/// assert_eq!(text, "ab");
/// assert!(confidence > 0.8);
/// ```
pub fn ctc_greedy_decode(
    logits: &[f32],
    width: usize,
    dict: &[String],
) -> Result<(String, f32), OcrError> {
    let num_classes = dict.len();
    if num_classes == 0 {
        return Err(OcrError::Postprocess(
            "recognition dictionary is empty".to_string(),
        ));
    }
    if logits.len() != width * num_classes {
        return Err(OcrError::Postprocess(format!(
            "logits length {} does not match width {} * classes {}",
            logits.len(),
            width,
            num_classes
        )));
    }

    let mut indices = Vec::new();
    let mut probabilities = Vec::new();
    let mut previous = usize::MAX;
    for time in 0..width {
        let row = &logits[time * num_classes..(time + 1) * num_classes];
        let (best, probability) = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| OcrError::Postprocess("logits row is empty".to_string()))?;
        if best != previous {
            indices.push(best);
            probabilities.push(*probability);
            previous = best;
        }
    }

    let mut text = String::new();
    let mut total = 0.0_f32;
    let mut count = 0_usize;
    for (index, probability) in indices.into_iter().zip(probabilities) {
        if index == 0 {
            continue;
        }
        if let Some(character) = dict.get(index) {
            text.push_str(character);
            total += probability;
            count += 1;
        }
    }
    let confidence = if count == 0 {
        0.0
    } else {
        total / count as f32
    };
    Ok((text, confidence))
}

/// Clamp polygon points to the resized-image bounds.
fn clip_polygon(polygon: &[Point; 4], width: usize, height: usize) -> [Point; 4] {
    let max_x = width.saturating_sub(1) as f32;
    let max_y = height.saturating_sub(1) as f32;
    polygon.map(|point| [point[0].clamp(0.0, max_x), point[1].clamp(0.0, max_y)])
}

/// Scale polygon points back to original image coordinates.
fn scale_polygon(
    polygon: &[Point; 4],
    ratio_x: f32,
    ratio_y: f32,
    original_width: u32,
    original_height: u32,
) -> [Point; 4] {
    let max_x = original_width.saturating_sub(1) as f32;
    let max_y = original_height.saturating_sub(1) as f32;
    polygon.map(|point| {
        [
            (point[0] / ratio_x).clamp(0.0, max_x),
            (point[1] / ratio_y).clamp(0.0, max_y),
        ]
    })
}

/// Compute the width and height of an ordered polygon.
fn box_dimensions(polygon: &[Point; 4]) -> (f32, f32) {
    let width = distance(polygon[0], polygon[1]);
    let height = distance(polygon[0], polygon[3]);
    (width, height)
}

fn distance(a: Point, b: Point) -> f32 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
}

fn center_x(box_: &DetectedBox) -> f32 {
    polygon_center(&box_.polygon)[0]
}

fn center_y(box_: &DetectedBox) -> f32 {
    polygon_center(&box_.polygon)[1]
}

/// Build an axis-aligned text box for tests.
#[cfg(test)]
fn text_box(x: f32, y: f32, width: f32, height: f32) -> DetectedBox {
    DetectedBox {
        polygon: [
            [x, y],
            [x + width, y],
            [x + width, y + height],
            [x, y + height],
        ],
        score: 1.0,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn boxes_from_map_finds_single_component() {
        let mut probability = Array2::<f32>::zeros((8, 8));
        for y in 2..6 {
            for x in 2..6 {
                probability[[y, x]] = 0.9;
            }
        }
        let params = DetectionParams {
            threshold: 0.5,
            unclip_ratio: 0.0,
            min_box_area: 3.0,
        };
        let boxes = boxes_from_map(&probability, params, 1.0, 1.0, 8, 8);
        assert_eq!(boxes.len(), 1);
        assert!((boxes[0].width - 4.0).abs() < 1.5);
        assert!((boxes[0].height - 4.0).abs() < 1.5);
        assert!((boxes[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn boxes_from_map_scales_coordinates() {
        let mut probability = Array2::<f32>::zeros((4, 4));
        probability[[1, 1]] = 0.9;
        probability[[1, 2]] = 0.9;
        probability[[2, 1]] = 0.9;
        probability[[2, 2]] = 0.9;
        let params = DetectionParams {
            threshold: 0.5,
            unclip_ratio: 0.0,
            min_box_area: 3.0,
        };
        let boxes = boxes_from_map(&probability, params, 0.5, 0.5, 8, 8);
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].polygon.iter().all(|p| p[0] <= 8.0 && p[1] <= 8.0));
        assert!((boxes[0].width - 2.0).abs() < 1.0);
    }

    #[test]
    fn boxes_from_map_drops_tiny_components() {
        let mut probability = Array2::<f32>::zeros((4, 4));
        probability[[1, 1]] = 0.9;
        let params = DetectionParams {
            threshold: 0.5,
            unclip_ratio: 0.0,
            min_box_area: 3.0,
        };
        assert!(boxes_from_map(&probability, params, 1.0, 1.0, 4, 4).is_empty());
    }

    #[test]
    fn horizontal_sort_reads_left_to_right_top_to_bottom() {
        let options = OcrOptions {
            detect_vertical: false,
            ..OcrOptions::default()
        };
        let boxes = vec![
            text_box(50.0, 20.0, 20.0, 8.0),
            text_box(0.0, 20.0, 20.0, 8.0),
            text_box(0.0, 0.0, 20.0, 8.0),
            text_box(40.0, 0.0, 20.0, 8.0),
        ];
        let sorted = sort_boxes(boxes, &options);
        let centers: Vec<[f32; 2]> = sorted.iter().map(|b| polygon_center(&b.polygon)).collect();
        assert_eq!(
            centers,
            vec![[10.0, 4.0], [50.0, 4.0], [10.0, 24.0], [60.0, 24.0]]
        );
    }

    #[test]
    fn vertical_sort_reads_top_to_bottom_right_to_left() {
        let options = OcrOptions {
            detect_vertical: true,
            ..OcrOptions::default()
        };
        let boxes = vec![
            text_box(100.0, 30.0, 8.0, 20.0),
            text_box(100.0, 0.0, 8.0, 20.0),
            text_box(0.0, 0.0, 8.0, 20.0),
            text_box(0.0, 30.0, 8.0, 20.0),
        ];
        let sorted = sort_boxes(boxes, &options);
        let centers: Vec<[f32; 2]> = sorted.iter().map(|b| polygon_center(&b.polygon)).collect();
        assert_eq!(
            centers,
            vec![[104.0, 10.0], [104.0, 40.0], [4.0, 10.0], [4.0, 40.0]]
        );
    }

    #[test]
    fn confidence_filter_keeps_at_or_above_threshold() {
        let lines = vec![
            RecognizedLine {
                text: "a".to_string(),
                confidence: 0.5,
            },
            RecognizedLine {
                text: "b".to_string(),
                confidence: 0.49,
            },
        ];
        assert_eq!(filter_by_confidence(lines, 0.5).len(), 1);
    }

    #[test]
    fn merge_lines_uses_reading_order() {
        let lines = vec![
            OcrLine::new("b", 1.0, [[0.0; 2]; 4], 1),
            OcrLine::new("a", 1.0, [[0.0; 2]; 4], 0),
            OcrLine::new("c", 1.0, [[0.0; 2]; 4], 2),
        ];
        assert_eq!(merge_lines(&lines), "a\nb\nc");
    }

    #[test]
    fn ctc_decode_merges_repeats_and_blanks() {
        let dict = ["", "a", "b"].map(String::from).to_vec();
        let logits = [
            0.1, 0.9, 0.0, 0.1, 0.9, 0.0, 0.9, 0.1, 0.0, 0.1, 0.0, 0.9, 0.9, 0.1, 0.0,
        ];
        let (text, confidence) = ctc_greedy_decode(&logits, 5, &dict).unwrap();
        assert_eq!(text, "ab");
        assert!(confidence > 0.8);
    }

    #[test]
    fn ctc_decode_keeps_repeat_separated_by_blank() {
        let dict = ["", "a"].map(String::from).to_vec();
        let logits = [0.1, 0.9, 0.9, 0.1, 0.1, 0.9];
        let (text, _) = ctc_greedy_decode(&logits, 3, &dict).unwrap();
        assert_eq!(text, "aa");
    }

    #[test]
    fn ctc_decode_rejects_mismatched_length() {
        let dict = ["", "a"].map(String::from).to_vec();
        assert!(matches!(
            ctc_greedy_decode(&[0.1, 0.9], 3, &dict),
            Err(OcrError::Postprocess(_))
        ));
    }
}
