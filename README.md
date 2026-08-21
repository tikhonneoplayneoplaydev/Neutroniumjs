# Neutronium.js ⚡

Быстрый модульный JavaScript runtime на Rust. Проект даёт компактное ядро, CLI
`neut`, REPL, безопасный слой расширений, рабочий **Cranelift executable JIT**,
изолированный **LLVM**-бэкенд, собственный формат модулей **`.nim`** и вызовы
C ABI через FFI.

## Быстрый старт

```bash
git clone https://github.com/tikhonneoplayneoplaydev/Neutroniumjs.git
cd Neutroniumjs
cargo build --release
cargo install --path .
neut examples/hello.js
```

## Возможности

```bash
neut run app.js                       # запустить JavaScript-файл (Boa)
neut repl                             # интерактивная консоль
neut info                             # версия и активный backend
neut jit "(1+2)*3"                    # исполнить выражение через Cranelift JIT
neut ffi ./libhello.so neut_init      # проверить C-символ в нативной библиотеке
neut load ./libhello.so               # загрузить нативный модуль (stable ABI)

# .nim — собственный контейнер модулей
neut pack ./mymod --name mymod --version 1.0.0
neut unpack ./mymod.nim --out ./out
neut manifest ./mymod.nim
```

### `.nim`-модули

`.nim` — это простой детерминированный контейнер: магическая строка `NIM1`,
JSON-манифест (`name`, `version`, `main`, `dependencies`, …) и список файлов с
относительными путями. Пуси нормализуются, `..` и абсолютные пути запрещены,
запись идёт в отсортированном порядке, поэтому упаковка одного и того же
каталога всегда даёт идентичные байты. API находится в [`src/nim.rs`](src/nim.rs).

### JIT/AOT бэкенды

- **Cranelift JIT** (`cranelift` feature, включён по умолчанию): реальный
  исполняемый JIT в [`src/cranelift_backend.rs`](src/cranelift_backend.rs).
  Парсит арифметическое выражение, опускает его в Cranelift IR, компилирует в
  машинный код и вызывает через `extern "C"` указатель. Никаких внешних
  тулчейнов в рантайме не требуется.
- **LLVM** (`llvm` feature, опционально): полностью изолирован в
  [`src/llvm_backend.rs`](src/llvm_backend.rs) и подключается через `inkwell`.
  Остальной код о LLVM ничего не знает. Сборка требует системного LLVM:

  ```bash
  cargo check --no-default-features --features "llvm/llvm-17"
  ```

Трейт бэкенда [`CompilerBackend`](src/backend.rs) отделяет хост (Boa) от
генераторов кода, поэтому новые бэкенды добавляются без изменений CLI.

### FFI / C ABI

Весь `unsafe`-код собран в [`src/ffi.rs`](src/ffi.rs). Указатели одалживаются
только на время вызова, Rust не владеет нативной памятью. ABI описан в
[`native/neutronium.h`](native/neutronium.h).

## Разработка

```bash
cargo fmt --all
cargo check --all-targets
cargo test
```

Лицензия MIT.
