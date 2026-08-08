# NOTICE

Copyright (c) 2026 VTrans Team.

This application bundles or links against third-party components whose
notices must be preserved. The text below is the engineering-level license
registration for the translation stack (decision B6); official license
texts are referenced by URL. Formal commercial distribution requires a
legal review of the concrete model artifacts (integration guide §24).

## 1. Bergamot Translator — MPL-2.0

- Repository: <https://github.com/browsermt/bergamot-translator>
- Version: v0.4.5
- License: Mozilla Public License 2.0 (SPDX: MPL-2.0)
- Official text: <https://www.mozilla.org/en-US/MPL/2.0/>
- Used for: local en→zh translation engine (`translation_bridge.dll`)
- Copyright: (c) Mozilla Foundation and contributors

## 2. CTranslate2 — MIT

- Repository: <https://github.com/OpenNMT/CTranslate2>
- Version: 4.8.1
- License: MIT (SPDX: MIT)
- Official text: <https://github.com/OpenNMT/CTranslate2/blob/v4.8.1/LICENSE>
- Used for: local ja→zh translation engine (`translation_bridge.dll`)
- Copyright: (c) 2017-2024 OpenNMT (MIT License, see `CTranslate2-MIT.txt`)

## 3. MarianMT model (shun89/opus-mt-ja-zh) — Apache-2.0

- Repository: <https://huggingface.co/shun89/opus-mt-ja-zh>
- Revision: 0728b51b9be02330f7bce262a4d47f611fd3a2a4 (from the v2 manifest)
- License: Apache License 2.0 (SPDX: Apache-2.0), as marked on the model card
- Official text: <https://www.apache.org/licenses/LICENSE-2.0>
- Used for: ja→zh model weights (converted to CTranslate2 INT8)

## 4. SentencePiece — Apache-2.0

- Repository: <https://github.com/google/sentencepiece>
- Version: pinned revision (see `native/translation_bridge/README.md`)
- License: Apache License 2.0 (SPDX: Apache-2.0)
- Official text: <https://www.apache.org/licenses/LICENSE-2.0>
- Used for: ja/zh subword encode/decode in `translation_bridge.dll`
- Copyright: (c) 2016 Google Inc.

## 5. Mozilla en-zh translation model

- Registry: <https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json>
- Selected entry: en-zh Release, architecture `base-memory`
  (registryGenerated 2026-08-07T00:43:32Z)
- License: check the model artifact's own license/registry record before
  commercial redistribution (integration guide §24); do not infer the
  model license from the engine license.
- Used for: en→zh model weights (Bergamot package)

## 6. Workspace license

VTrans itself is licensed under MIT OR Apache-2.0 (see workspace
`Cargo.toml`). This notice does not replace any upstream license text;
full texts of the short licenses are included in this directory for
convenience:

- `CTranslate2-MIT.txt`

MPL-2.0 and Apache-2.0 texts are long-form standards; please use the
official URLs above when redistributing.
