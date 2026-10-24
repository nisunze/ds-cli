# Frozen opened Solar fixtures

These plain JSON tuples contain weather and the sealed reference unit, including
all 8,760 hourly values. They were exported once from ds-solar revision 8344cfffddce849595ce19281cb3cd9267c163bf
using its owning reference decoder; Core tests only deserialize the values.

PROVENANCE.json records every raw-source digest, tuple digest, copied city
snapshot digest and hourly f64 bit digest. The exporter checked complete weather
serialization and every hourly profile value by to_bits() before writing. No
engine, source-tree decoder or executable probe is required to construct these
test inputs. Actual addon admission and process tests remain separate.
