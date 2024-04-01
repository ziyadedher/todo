`todo` is a library and command line tool for interacting with Asana.

# Usage

## Command line interface

Install the command line tool with `cargo install --path .` while in the `todo` codebase, then run
`todo --help` for usage information.

### Updating automatically in the background

If you're using the `--use-cache` option, we'll pull from the most recent cache (which gets updated when you run `todo update` or run any other commands). But if you're not actively using the tool, and only using it e.g. at terminal startup to show you how many tasks you have, then you probably want that cache to be up to date all the time. You can easily set up a cronjob on your Mac to do that.

Open up `crontab -e` and add the following line:
```
*/1 * * * * . "$HOME/.cargo/env" && todo update
```

If caching isn't working, you'll get a warning.

## Library

Add the `todo` codebase to your `Cargo.toml` (you can point to local files) and then check out
`[lib.rs](src/lib.rs)` for documentation. You can also build and read the documentation for the library by running
`cargo doc --open` while in the `todo` codebase
