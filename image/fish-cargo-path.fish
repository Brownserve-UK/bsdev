# Added by the bsdev image: fish doesn't add ~/.cargo/bin to PATH automatically,
# so the cargo/rustc/rustfmt/clippy shims rustup installs there need this to be
# found. Lives in /etc/fish/conf.d, which fish sources for every session.
if test -d $HOME/.cargo/bin
    fish_add_path -g $HOME/.cargo/bin
end
