# Consumer-owned test fixtures

These are exact-byte snapshots of the named producer fixtures. Existing tests
own these copies so compiling and replaying them does not read sibling source
trees. Update deliberately with the producer contract; retain negative controls.

| Local fixture | Producer path | Bytes | SHA256 | Producer file commit |
| --- | --- | ---: | --- | --- |
| native-engine-boundary/v1/mixed.request.json | ds-network/crates/ds-network/tests/fixtures/native-engine-boundary/v1/mixed.request.json | 872 | b91ef5ebc472d50ebcddcf1f67fedaa6d2fa0a00a5027b7ea17eb3d64a6a178a | e641e3d696ad9bd3f7b4cf6b7eb54ad62026231a |
| native-engine-boundary/v1/mixed.insertion.result.json | ds-network/crates/ds-network/tests/fixtures/native-engine-boundary/v1/mixed.insertion.result.json | 13150 | f53049bc9508a417e706d18a9c064f41479c119241a56072534fa53ec7adb2ae | 056687b7b9da28192a6d1cc3cdd6d5a825a46812 |
| lv-standard/a4-document.json | ds-network-reporter/tests/fixtures/lv-standard/a4-document.json | 197133 | b4a283b107bc1121af7a39736b544117e3e961b111a77937af1fcfc439c5a6ab | b28c844a08a2f27e072ce7de888524cca7eafeb2 |

The LV result is the insertion-order golden. Its exact bytes and original
request/result digests remain the acceptance oracle. The A4 document is cloned
inside tests for missing/crossed-project and export-failure controls.
