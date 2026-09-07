# Rwanda reference catalog

`ds desktop reference rwanda status` refreshes the active project's governed
Brain catalog and proves the exact version and local spatial index of every
published Rwanda resource.

`ds desktop reference rwanda seed` downloads every available resource as an
exact-size, digest-pinned compressed bundle, expands it into bounded native
pages, and builds its SQLite RTree. Repeat
`--resource <sha256-id>` to install a selected subset. The command does not
contain a layer list: roads, settlements, schools, infrastructure and later
published Rwanda resources enter the next run through the catalog. Completed
resources are retained when a later download is interrupted and are verified
again when the command resumes. Status reports the actual expanded cache,
spatial-index and total disk bytes; the catalog shown by Desktop supplies the
compressed network and expanded-size estimates before installation. Missing or
deliberately uninstalled resources remain visible in the receipt and do not
block mapping or printing.

The paired Desktop owns the local store and the signed-in application owns the
Brain request. No token or raw credential crosses the loopback bridge. Cloud
queries keep using Brain's native BigQuery tables and exact polygon
intersection. Local queries require the matching completed install. Neither
provider falls back to the other.
