# Proton Pass CLI

A command-line interface for [Proton Pass](https://proton.me/pass).

## Documentation

Full documentation is available at **https://protonpass.github.io/pass-cli/**

## Installing the custom build (Windows)

This fork publishes a ready-to-use Windows binary in its GitHub releases. Install it with:

```powershell
irm https://raw.githubusercontent.com/Winson-NoOT/pass-cli/main/install-custom.ps1 | iex
```

The script installs `pass-cli.exe` to `%LOCALAPPDATA%\Programs\ProtonPass` and adds that
directory to your user `PATH`.

> **Note:** running `pass-cli update` installs the official upstream release and removes the
> custom `item search` command. Re-run the installer above to restore the custom build.

## Building from source

**Prerequisites:** [Rust toolchain](https://rustup.rs/) (stable)

```bash
# Debug build
cargo build

# Release (optimized) build
cargo build --release
```

The compiled binary will be at `target/debug/pass-cli` or `target/release/pass-cli` respectively.

## Development

### Running tests

```bash
cargo test
```

To run tests for a specific crate:

```bash
cargo test -p pass-cli
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

The code and data files in this distribution are licensed under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. See <https://www.gnu.org/licenses/> for a copy of this license.

See [LICENSE](LICENSE) file