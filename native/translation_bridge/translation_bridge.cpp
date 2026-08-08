// translation_bridge.cpp - C++17 implementation of the C ABI in
// translation_bridge.h.
//
// Wraps three engines behind a tiny C interface:
//   * Bergamot (browsermt/bergamot-translator v0.4.5)  for en -> zh
//   * CTranslate2 (OpenNMT/CTranslate2 4.8.1)          for ja -> zh
//   * SentencePiece (google/sentencepiece, pinned revision) for ja/zh
//     subword encoding/decoding
//
// Threading (integration guide sections 16/25, decision B5):
//   * Bergamot is pinned to 2 CPU threads (config.cpuThreads = 2).
//   * CTranslate2 uses intra_threads = 2 and inter_threads = 1 (a single
//     in-process Translator instance; no thread pool is created).
//   * The engine mutex serializes all bridge calls so the total concurrency
//     never grows with the number of CPU cores.
//
// Text handling (integration guide section 20): every string that crosses
// the C boundary is UTF-8. Windows ACP/ANSI APIs are never used.

#include "translation_bridge.h"

#include <cstring>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

// Bergamot v0.4.5 public headers.
#include "translator/annotated_text.h"
#include "translator/response.h"
#include "translator/service.h"
#include "translator/translation_model.h"

// CTranslate2 4.8.1 public headers.
#include <ctranslate2/models/model.h>
#include <ctranslate2/translation_result.h>
#include <ctranslate2/translator.h>

// SentencePiece public header (pinned revision, see README.md).
#include <sentencepiece_processor.h>

namespace {

constexpr int kBridgeAbiVersion = TRANSLATION_BRIDGE_ABI_VERSION;

// Quality presets (kept in sync with the Rust `TranslationQuality` mapping
// in crates/vtrans-translation/src/native.rs).
constexpr int kBeamBergamotFast = 1;
constexpr int kBeamBergamotBalanced = 2;
constexpr int kBeamCt2Fast = 1;
constexpr int kBeamCt2Balanced = 4;
constexpr int kCt2MaxInputTokens = 256;
constexpr int kCt2MaxDecodingLength = 256;

// Bergamot threading budget (B5): never auto-grow with core count.
constexpr size_t kBergamotCpuThreads = 2;

// CTranslate2 threading budget (B5): intra 2 / inter 1.
constexpr int kCt2IntraThreads = 2;

// Translate a C string into a std::string, treating it as raw UTF-8 bytes.
// Returns false when the pointer is null.
bool cstr_to_string(const char* input, std::string& out) {
    if (input == nullptr) {
        return false;
    }
    out.assign(input);
    return true;
}

// Copy a UTF-8 std::string into a malloc-compatible buffer the caller
// releases with translation_free_string(). Returns null on failure.
char* allocate_output(const std::string& text) {
    const size_t size = text.size() + 1;  // +1 for the NUL terminator.
    char* buffer = new (std::nothrow) char[size];
    if (buffer == nullptr) {
        return nullptr;
    }
    std::memcpy(buffer, text.data(), text.size());
    buffer[text.size()] = '\0';
    return buffer;
}

// Map a std::exception to the closest TR_ERR_* code. Tokenizer failures are
// detected by message content because SentencePiece throws generic
// runtime_error/status exceptions; everything else is an inference failure.
int exception_to_error_code(const std::exception& error) {
    const std::string what = error.what();
    if (what.find("spm") != std::string::npos || what.find("SentencePiece") != std::string::npos) {
        return TR_ERR_TOKENIZER;
    }
    return TR_ERR_INFERENCE;
}

// The engine owns every model object for the process lifetime
// (integration guide section 15). A single mutex serializes all bridge
// calls, keeping the total native concurrency bounded.
struct EngineImpl {
    std::mutex mutex;

    // ja -> zh pipeline.
    std::shared_ptr<ctranslate2::models::Model> ct2_model;
    std::unique_ptr<ctranslate2::Translator> ct2_translator;
    std::unique_ptr<sentencepiece::SentencePieceProcessor> ja_spm;
    std::unique_ptr<sentencepiece::SentencePieceProcessor> zh_spm;
    int ct2_beam = kBeamCt2Fast;

    // en -> zh pipeline (Bergamot). The beam size is fixed at model
    // construction time, so switching quality rebuilds the model. Quality
    // changes happen once at provider assembly, not per request.
    std::string bergamot_model_path;
    std::string bergamot_src_vocab;
    std::string bergamot_trg_vocab;
    std::string bergamot_shortlist;
    std::unique_ptr<marian::bergamot::TranslationModel> bergamot_model;
    std::unique_ptr<marian::bergamot::Service> bergamot_service;
    int bergamot_beam = kBeamBergamotFast;
};

// Build the Bergamot pipeline for the current beam size. Throws on failure.
// The model and service are constructed into locals first and only moved
// into the engine after both succeeded, so a failed rebuild never leaves a
// half-updated pipeline behind.
void rebuild_bergamot(EngineImpl& engine) {
    marian::bergamot::TranslationModel::Config config;
    config.model = engine.bergamot_model_path;
    // v0.4.5 takes one entry per vocabulary file (source, target).
    config.vocabulary = {engine.bergamot_src_vocab, engine.bergamot_trg_vocab};
    config.shortlist = engine.bergamot_shortlist;
    config.beam = static_cast<size_t>(engine.bergamot_beam);
    config.normalize = 1.0F;
    config.wordPenalty = 0;
    config.maxLengthBreak = 128;
    config.miniBatchWords = 1024;
    config.workspaceSize = 128;
    config.maxLengthFactor = 2.0F;
    config.skipCost = true;
    config.cpuThreads = kBergamotCpuThreads;
    config.gemmPrecision = "int8shiftAlphaAll";
    config.quiet = true;
    config.quietTranslation = true;

    marian::bergamot::AsyncService::Options service_options;
    service_options.numWorkers = 1;
    auto model = std::make_unique<marian::bergamot::TranslationModel>(config);
    auto service = std::make_unique<marian::bergamot::Service>(*model, service_options);
    engine.bergamot_model = std::move(model);
    engine.bergamot_service = std::move(service);
}

// Translate with the Bergamot en -> zh pipeline. Throws on failure.
std::string translate_bergamot(EngineImpl& engine, const std::string& input) {
    marian::bergamot::AnnotatedText source;
    source.text = input;
    marian::bergamot::Response response = engine.bergamot_service->translate(std::move(source)).get();
    return response.target.text;
}

// Translate with the CTranslate2 ja -> zh pipeline (SentencePiece encode ->
// model -> SentencePiece decode). Throws on failure.
std::string translate_ct2(EngineImpl& engine, const std::string& input) {
    std::vector<std::string> source_tokens;
    if (!engine.ja_spm->Encode(input, &source_tokens).ok()) {
        throw std::runtime_error("spm encode failed");
    }

    ctranslate2::TranslationOptions options;
    options.beam_size = engine.ct2_beam;
    options.max_input_length = kCt2MaxInputTokens;
    options.max_decoding_length = kCt2MaxDecodingLength;

    const std::vector<std::vector<std::string>> batch{std::move(source_tokens)};
    const std::vector<ctranslate2::TranslationResult> results =
        engine.ct2_translator->translate_batch(batch, options);
    if (results.empty() || results[0].hypotheses.empty()) {
        throw std::runtime_error("ctranslate2 returned no hypothesis");
    }

    const std::vector<std::string>& target_tokens = results[0].hypotheses[0];
    std::string output;
    if (!engine.zh_spm->Decode(target_tokens, &output).ok()) {
        throw std::runtime_error("spm decode failed");
    }
    return output;
}

}  // namespace

extern "C" {

TR_API int translation_bridge_version(void) {
    return kBridgeAbiVersion;
}

TR_API TranslationEngine translation_create(
    const char* enzh_model_dir,
    const char* jazh_model_dir) {
    std::string enzh_dir;
    std::string jazh_dir;
    if (!cstr_to_string(enzh_model_dir, enzh_dir) || !cstr_to_string(jazh_model_dir, jazh_dir) ||
        enzh_dir.empty() || jazh_dir.empty()) {
        return nullptr;
    }

    std::unique_ptr<EngineImpl> engine = std::make_unique<EngineImpl>();
    try {
        // CTranslate2 ja -> zh: model + config + vocab + source/target SPM.
        engine->ja_spm = std::make_unique<sentencepiece::SentencePieceProcessor>();
        if (!engine->ja_spm->Load(jazh_dir + "/source.spm").ok()) {
            throw std::runtime_error("spm load failed: source.spm");
        }
        engine->zh_spm = std::make_unique<sentencepiece::SentencePieceProcessor>();
        if (!engine->zh_spm->Load(jazh_dir + "/target.spm").ok()) {
            throw std::runtime_error("spm load failed: target.spm");
        }
        engine->ct2_model = ctranslate2::models::Model::load(
            jazh_dir, ctranslate2::Device::CPU, /*device_index=*/0,
            ctranslate2::ComputeType::INT8);
        engine->ct2_translator = std::make_unique<ctranslate2::Translator>(engine->ct2_model);
        engine->ct2_beam = kBeamCt2Fast;

        // Bergamot en -> zh: model + src/trg vocab + lexical shortlist.
        engine->bergamot_model_path = enzh_dir + "/model.enzh.intgemm.alphas.bin";
        engine->bergamot_src_vocab = enzh_dir + "/srcvocab.enzh.spm";
        engine->bergamot_trg_vocab = enzh_dir + "/trgvocab.enzh.spm";
        engine->bergamot_shortlist = enzh_dir + "/lex.50.50.enzh.s2t.bin";
        engine->bergamot_beam = kBeamBergamotFast;
        rebuild_bergamot(*engine);
    } catch (const std::exception&) {
        return nullptr;
    }

    // Pin the intra-thread budget before any request is served.
    ctranslate2::set_num_threads(kCt2IntraThreads);
    return engine.release();
}

TR_API int translation_set_quality(TranslationEngine handle, const char* quality) {
    if (handle == nullptr) {
        return TR_ERR_MODEL_NOT_LOADED;
    }
    std::string quality_str;
    if (!cstr_to_string(quality, quality_str)) {
        return TR_ERR_INVALID_ARGUMENT;
    }

    EngineImpl& engine = *static_cast<EngineImpl*>(handle);
    std::lock_guard<std::mutex> lock(engine.mutex);

    int bergamot_beam = 0;
    int ct2_beam = 0;
    if (quality_str == "fast") {
        bergamot_beam = kBeamBergamotFast;
        ct2_beam = kBeamCt2Fast;
    } else if (quality_str == "balanced") {
        bergamot_beam = kBeamBergamotBalanced;
        ct2_beam = kBeamCt2Balanced;
    } else {
        return TR_ERR_INVALID_ARGUMENT;
    }

    try {
        if (bergamot_beam != engine.bergamot_beam) {
            const int previous_beam = engine.bergamot_beam;
            engine.bergamot_beam = bergamot_beam;
            try {
                rebuild_bergamot(engine);
            } catch (...) {
                engine.bergamot_beam = previous_beam;
                throw;
            }
        }
        engine.ct2_beam = ct2_beam;
    } catch (const std::exception&) {
        return TR_ERR_MODEL_NOT_LOADED;
    }
    return TR_OK;
}

TR_API int translation_translate(
    TranslationEngine handle,
    const char* source_lang,
    const char* utf8_input,
    char** utf8_output) {
    if (handle == nullptr || utf8_output == nullptr) {
        return TR_ERR_INVALID_ARGUMENT;
    }

    std::string language;
    std::string input;
    if (!cstr_to_string(source_lang, language) || !cstr_to_string(utf8_input, input)) {
        return TR_ERR_INVALID_ARGUMENT;
    }
    if (input.empty()) {
        return TR_ERR_INVALID_ARGUMENT;
    }

    EngineImpl& engine = *static_cast<EngineImpl*>(handle);
    std::lock_guard<std::mutex> lock(engine.mutex);

    try {
        std::string output;
        if (language == "en") {
            if (engine.bergamot_model == nullptr || engine.bergamot_service == nullptr) {
                return TR_ERR_MODEL_NOT_LOADED;
            }
            output = translate_bergamot(engine, input);
        } else if (language == "ja") {
            if (engine.ct2_translator == nullptr || engine.ja_spm == nullptr ||
                engine.zh_spm == nullptr) {
                return TR_ERR_MODEL_NOT_LOADED;
            }
            output = translate_ct2(engine, input);
        } else {
            return TR_ERR_UNSUPPORTED_LANGUAGE;
        }

        char* buffer = allocate_output(output);
        if (buffer == nullptr) {
            return TR_ERR_ENCODING;
        }
        *utf8_output = buffer;
        return TR_OK;
    } catch (const std::exception& error) {
        return exception_to_error_code(error);
    }
}

TR_API void translation_free_string(char* ptr) {
    delete[] ptr;
}

TR_API void translation_destroy(TranslationEngine handle) {
    if (handle == nullptr) {
        return;
    }
    // No lock is taken here: the Rust side drops the wrapper only after the
    // last shared reference is gone, so no in-flight translate/set_quality
    // call can race this destruction.
    EngineImpl& engine = *static_cast<EngineImpl*>(handle);
    delete &engine;
}

}  // extern "C"
