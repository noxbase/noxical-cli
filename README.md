# CLI tools for Noxical

TEST

1. Compile the Rust program:

```bash
cargo build --release
```

This creates an executable under `./target/release/noxical` (or `noxical.exe` in Windows).

2. Run the executable:

```bash
./target/release/noxical --input my_input_folder
```

Optionally, you can add the `--watch` flag to re-compile whenever any file in the given input folder changes:

```bash
./target/release/noxical --input my_input_folder --watch
```
