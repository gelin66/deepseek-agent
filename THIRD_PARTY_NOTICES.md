# Third-party notices and provenance

## CodeWhale source baseline

DSE continues from the CodeWhale source imported at commit
`352e86a611fdf3cd8bd27c36d24d482c06a71117` from
[Hmbown/CodeWhale](https://github.com/Hmbown/CodeWhale).

That imported source is licensed under the MIT License. DSE preserves the
repository [LICENSE](LICENSE); its SHA-256 at the M17-G audit is
`91873e17f073f4dcddc63799a0a6fdeb44a281440b6c5e0b9d8ea2aa7f7ffd95`, byte-for-byte
equal to the license at the imported baseline.

DSE modifications after the import remain in the repository's Git history.
Current DSE identity does not rewrite CodeWhale provenance, historical schema
names, frozen evaluation artifacts, or commit history.

## Rust and bundled dependencies

Third-party dependencies retain their own copyright and license terms. The
locked dependency graph is recorded in `Cargo.lock`; distributors are
responsible for satisfying the licenses of the exact dependencies and any
platform libraries included in a binary or package. `Cargo.lock` is an
identity record, not a substitute for those licenses.

Projects mentioned in architecture or evaluation documents are capability
references unless a file explicitly states that source or assets were
incorporated. A reference alone does not add that project's runtime, provider,
protocol, trademark, or license grant to DSE.

## DeepSeek services and names

DSE calls the official DeepSeek API but does not bundle the DeepSeek service or
model weights. DSE is an independent community project and is not affiliated
with, sponsored by, or endorsed by DeepSeek. DeepSeek names, models, APIs, and
trademarks belong to their respective owners.
