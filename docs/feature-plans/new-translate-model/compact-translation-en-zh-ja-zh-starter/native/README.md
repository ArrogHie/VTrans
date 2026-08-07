# Native bridge

Implement this bridge in C++ and link:

- browsermt/bergamot-translator
- OpenNMT/CTranslate2
- SentencePiece

Keep the ABI tiny. Do not expose C++ STL containers directly to Rust/Node.

Suggested internal members:

```cpp
struct TranslationEngineImpl {
  // Bergamot en->zh model/service
  // CTranslate2 ja->zh Translator
  // SentencePiece source/target processors
};
```

The bridge should own all model objects for the process lifetime.
