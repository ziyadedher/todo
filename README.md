`todo` is a library and command line tool for interacting with Asana.

# Usage

## Command line interface

Install from crates.io:
```sh
cargo install ziyad-todo
```

Or use [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) for pre-built binaries:
```sh
cargo binstall ziyad-todo
```

Then run `todo --help` for usage information.

### Updating automatically in the background

If you're using the `--use-cache` option, we'll pull from the most recent cache (which gets updated when you run `todo update` or run any other commands). But if you're not actively using the tool, and only using it e.g. at terminal startup to show you how many tasks you have, then you probably want that cache to be up to date all the time. You can easily set up a cronjob to do that.

Open up `crontab -e` and add the following line:
```sh
*/1 * * * * . "$HOME/.cargo/env" && todo update
```

If caching isn't working, you'll get a warning.

## Library

Add to your `Cargo.toml`:
```toml
[dependencies]
ziyad-todo = "1"
```

See [docs.rs/ziyad-todo](https://docs.rs/ziyad-todo) for documentation.
