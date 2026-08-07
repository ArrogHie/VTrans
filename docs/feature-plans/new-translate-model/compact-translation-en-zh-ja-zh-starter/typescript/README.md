# TypeScript frontend

For Tauri, TypeScript should call a Rust command over IPC. The Rust backend owns
Bergamot and CTranslate2.

For Electron, use the same idea:

renderer -> IPC -> main/native addon -> translation bridge

Do not load multiple copies of the models in multiple renderer windows.
