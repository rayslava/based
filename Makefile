# Can't define them conditionally via .cargo/config.toml
RELEASE_FLAGS="-C linker=rust-lld -C linker-flavor=ld.lld -C link-arg=--entry=_start -C link-arg=-nostdlib -C link-arg=-static -C link-arg=-no-pie -C link-arg=-S -C link-arg=-n -C link-arg=--strip-all -C link-arg=--discard-all -C link-arg=--discard-locals"

all: release

# Target is mandatory
# See https://github.com/rust-lang/compiler-builtins/issues/361#issuecomment-1011559018
release:
	RUSTFLAGS=$(RELEASE_FLAGS) cargo build --target x86_64-unknown-linux-gnu --release
	objcopy --remove-section=.eh_frame --remove-section=.shstrtab --remove-section=.comment target/x86_64-unknown-linux-gnu/release/based based

test:
	cargo nextest run

coverage:
	cargo tarpaulin --all-targets --count --force-clean --all-features --workspace --out Xml --engine Llvm

clean:
	rm -f based
	cargo clean

check:
	cargo check
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -W clippy::pedantic

fix:
	cargo fmt --all
	cargo fix --all --all-features --allow-dirty --allow-staged
	cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -W clippy::pedantic

size: release
	wc -c based
