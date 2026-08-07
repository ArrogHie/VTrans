export type SourceLanguage = "en" | "ja";

export interface TranslationResult {
  text: string;
  sourceLanguage: SourceLanguage;
}

// Tauri example. Keep native model/runtime details behind the Rust backend.
export async function translateText(
  sourceLanguage: SourceLanguage,
  text: string
): Promise<TranslationResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  const translated = await invoke<string>("translate_text", {
    sourceLanguage,
    text,
  });
  return { text: translated, sourceLanguage };
}
