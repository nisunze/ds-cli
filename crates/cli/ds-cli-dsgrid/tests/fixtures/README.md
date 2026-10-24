# Consumer-owned test fixtures

These are exact-byte snapshots of the named producer fixtures. Existing tests
own these copies so compiling and replaying them does not read sibling source
trees. Update deliberately with the producer contract; retain negative controls.

| Local fixture | Producer path | Bytes | SHA256 | Producer file commit |
| --- | --- | ---: | --- | --- |
| humble-pole.dsgrid | ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid | 78141 | aa185c9d2a0ac8d170076266d3c69aed5ed7472907c49643ddd2ccfbbe87ec92 | 730a00678096e1d4d8dd3142fbef1ad15d95cd72 |
| native-engine-boundary/v1/model.dsgrid | ds-network/crates/ds-grid-exchange/tests/fixtures/native-engine-boundary/v1/model.dsgrid | 33926 | d324ab6ce0caafcc23350c39e906c70afb4a9601036d2550f902e6015e6b6e75 | e641e3d696ad9bd3f7b4cf6b7eb54ad62026231a |
| ground-basis-schema4/schema4.dsgrid | ds-network/crates/ds-grid-exchange/tests/fixtures/ground-basis-schema4/schema4.dsgrid | 28970 | 7ccadc264cf9225ba87ae984bd17b5e1e218ca39c14f6831ec3c17165112b21d | 6b34bf3878d5e5a1ab70eb7648b2dd9fdd4330b9 |

The native boundary model is used both after the owning migration and as the
valid-but-unconfigured refusal input. The historical schema4 package retains its
original bytes for read-through migration and identity/corruption controls. The
humble-pole model retains the modulus-advisory oracle.
