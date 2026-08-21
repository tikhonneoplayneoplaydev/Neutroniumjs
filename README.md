# Neutronium.js ⚡

Быстрый модульный JavaScript runtime на Rust. Проект даёт компактное ядро, CLI `neut`, REPL, безопасный слой расширений и основу для подключаемых JIT/AOT бэкендов (Cranelift и LLVM), а также вызовов C ABI через FFI.

## Быстрый старт

```bash
git clone https://github.com/tikhonneoplayneoplaydev/Neutroniumjs.git
cd Neutroniumjs
cargo build --release
# Linux/macOS:
cargo install --path .
# или добавьте бинарник в PATH:
export PATH="$PWD/target/release:$PATH"
neut hello.js
```

Для постоянной установки (рекомендуется) `cargo install --path .` сам помещает `neut` в Cargo bin (`~/.cargo/bin`), который обычно уже находится в PATH. Windows: `cargo install --path .`, затем откройте новый терминал.

```bash
neut run app.js       # запуск файла
neut repl             # интерактивная консоль
neut info             # версия и активный backend
neut build --backend jit
neut ffi ./libmath.so add
```

## Архитектура и roadmap

- **Core**: Rust API и минимальный CLI, изолированный runtime-контекст.
- **JIT/AOT**: backend trait готов для подключения Cranelift/LLVM без изменения CLI; сборка по умолчанию не тащит тяжёлые toolchain-зависимости.
- **FFI/C ABI**: модуль `ffi` проверяет наличие C-символа в `.so`, `.dylib` или `.dll`; безопасные объявления функций рекомендуется оборачивать в Rust-модули.
- **Modules**: ES-модули и npm-совместимый registry — следующий слой, планируется в `modules/`.

> Сейчас JS исполняется встроенным Boa engine. LLVM/Cranelift интерфейс обозначен feature-флагами и будет развиваться отдельными backend-крейтами; это позволяет собрать и запустить проект без LLVM SDK.

## Разработка

```bash
cargo test
cargo fmt
cargo clippy --all-targets --all-features
```

Лицензия MIT.
