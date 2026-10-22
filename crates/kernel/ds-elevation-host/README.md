# Independent elevation comparison

`ds-elevation-host` owns the bounded native I/O and CSV comparison between the
governed Rwanda TIFF and AWS Terrarium. The CLI calls `compare_file`; it does
not reproduce projection, pixel sampling, or elevation arithmetic.

The CSV supplies explicit X/Y columns and a characterized source CRS. All
input fields remain in the output, including IDs, stations and alignment
names. Added columns give the actual comparison longitude/latitude, Rwanda
height, legacy Terrarium containing-pixel height, optional Terrarium bilinear
height, and each Terrarium-minus-Rwanda difference in metres.

Both providers are sampled independently at every identical coordinate.
Neither fills gaps in the other. Empty output cells represent missing samples
and differences. No model is opened or edited; no height correction or vertical
datum conversion is applied. Both comparison source datums remain explicitly
unknown. A raw difference is not evidence of an accepted datum adjustment.

Rwanda uses `ds-elevation`'s four-corner, pixel-centred bilinear sampler. The
public `2026-08-28` index is pinned by length and SHA-256. TIFF blocks require
exact HTTP 206, Content-Range total/object identity and byte counts. A server
ignoring Range is rejected before reading its body. No full national TIFF is
downloaded and no whole-object SHA-256 is invented.

Terrarium uses fixed public zoom-15 PNG tiles. The legacy control retains one
containing pixel. Bilinear sampling uses four pixel centres and follows
neighbouring tiles across boundaries; a transparent corner leaves the bilinear
sample missing. The nominal `30m` source label is separate from the zoom-15
pixel footprint and from input point spacing. Samples outside Mercator's
latitude domain are missing.

Admission is 4,000 points and 32 MiB input. Each PNG is limited to 2 MiB and
verified as 256 by 256 before decoding. TIFF blocks are limited to 32 MiB,
aggregate fetched bytes to 256 MiB, and decoded Terrarium holdings to 512 tiles.
No caller-provided URL or credentials are accepted. Every successful fetch has
its exact response length and SHA-256 in the returned receipt.

Provider or integrity failures abort before output creation. Outputs use
`create_new`, conflicting input/output column names are refused, and existing
files are never replaced. Offline fixture tests cover independent gaps,
transport failure, exact range validation, bilinear planes across tile
boundaries, transparency, input identity preservation, CRS conversion and
output collisions.
