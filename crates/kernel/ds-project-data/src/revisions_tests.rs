//! Seed once per project, proved with an in-memory door that keeps Brain's
//! own rules (one head per dataset, a revision admitted only against the
//! manifest it was planned on, delta outputs retained) and a fake source
//! that answers cells under the room's acquisition rule, counting every cell
//! it is asked for. Two roots are two machines; two principals are two members.
use super::*;
use crate::datasets::{buildings_dataset, contours_dataset, ContourSettings};
use ds_command_kernel::project_dataset_cache::{buffer_policy, Extent};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct Brain {
    head_revision: u64,
    outputs: Vec<(Output, Vec<u8>)>,
    reads: usize,
    published: usize,
    stale: usize,
    offline: bool,
    /// Another member's publication, landing just before ours.
    race: Option<Publication>,
}

#[derive(Clone)]
struct Door(Rc<RefCell<Brain>>);

impl Door {
    fn manifest_bytes(brain: &Brain) -> Option<Vec<u8>> {
        brain
            .outputs
            .iter()
            .find(|(output, _)| output.output_id == chain::MANIFEST_OUTPUT)
            .map(|(_, bytes)| bytes.clone())
    }

    fn land(brain: &mut Brain, publication: &Publication) -> Publish {
        let base = chain::base_fingerprint(Door::manifest_bytes(brain).as_deref());
        if base != publication.base_fingerprint {
            brain.stale += 1;
            return Publish::Stale {
                reason: "the current artifact head changed after this run opened".into(),
            };
        }
        for output in &publication.outputs {
            let bytes = std::fs::read(&output.path).unwrap();
            assert_eq!(bytes.len() as u64, output.size_bytes);
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), output.sha256);
            brain
                .outputs
                .retain(|(held, _)| held.output_id != output.output_id);
            brain.outputs.push((
                Output {
                    output_id: output.output_id.clone(),
                    sha256: output.sha256.clone(),
                    size_bytes: output.size_bytes,
                },
                bytes,
            ));
        }
        brain.head_revision += 1;
        brain.published += 1;
        Publish::Published {
            head_revision: brain.head_revision,
        }
    }
}

impl RevisionHost for Door {
    fn read(&mut self, _: &Head) -> Result<Option<Published>, SharedFailure> {
        let mut brain = self.0.borrow_mut();
        if brain.offline {
            return Err(SharedFailure::Project(Failure::ProviderUnavailable(
                "offline".into(),
            )));
        }
        brain.reads += 1;
        if brain.head_revision == 0 {
            return Ok(None);
        }
        Ok(Some(Published {
            head_revision: brain.head_revision,
            outputs: brain.outputs.iter().map(|(o, _)| o.clone()).collect(),
        }))
    }
    fn download(&mut self, _: &Head, output: &Output, dest: &Path) -> Result<(), SharedFailure> {
        let brain = self.0.borrow();
        let (_, bytes) = brain
            .outputs
            .iter()
            .find(|(held, _)| held.output_id == output.output_id)
            .expect("downloads name a published output");
        std::fs::write(dest, bytes).unwrap();
        Ok(())
    }
    fn publish(&mut self, publication: &Publication) -> Result<Publish, SharedFailure> {
        let mut brain = self.0.borrow_mut();
        if let Some(race) = brain.race.take() {
            assert!(matches!(
                Door::land(&mut brain, &race),
                Publish::Published { .. }
            ));
        }
        Ok(Door::land(&mut brain, publication))
    }
}

/// The publisher: fixed footprints, answered per cell under the room's rule.
struct Source {
    features: Vec<Value>,
    version: String,
    cells_asked: usize,
    calls: usize,
}

impl SourceHost for Source {
    fn current_version(&self, _: &Dataset) -> Option<String> {
        Some(self.version.clone())
    }
    fn acquire(
        &mut self,
        dataset: &Dataset,
        cells: &[[f64; 4]],
        scratch: &Path,
    ) -> Result<SourceBatch, SharedFailure> {
        self.calls += 1;
        self.cells_asked += cells.len();
        let mut pages = Vec::new();
        for (index, cell) in cells.iter().enumerate() {
            let mut rows = BTreeMap::new();
            for feature in &self.features {
                if acquisition_selects(&dataset.id, &dataset.provider, feature, &polygon(*cell))
                    .unwrap()
                {
                    let mut feature = feature.clone();
                    feature["properties"]["version"] = json!(self.version);
                    rows.insert(
                        feature["id"].as_str().unwrap().to_owned(),
                        feature.to_string(),
                    );
                }
            }
            let path = scratch.join(format!("page-{}-{index}.geojsonl", self.calls));
            let text: String = rows.values().map(|row| format!("{row}\n")).collect();
            std::fs::write(&path, text).unwrap();
            pages.push((*cell, path));
        }
        Ok(SourceBatch {
            source_version: self.version.clone(),
            receipt: json!({"publisher": "fixture", "version": self.version}),
            pages,
            uncovered: Vec::new(),
        })
    }
}

fn footprint(id: &str, x: f64, y: f64) -> Value {
    json!({"type":"Feature","id":id,"properties":{"actual_source":"google_open_buildings"},
        "geometry":{"type":"Polygon","coordinates":[[[x,y],[x+0.0001,y],[x+0.0001,y+0.0001],[x,y+0.0001],[x,y]]]}})
}

fn publisher(version: &str) -> Source {
    let mut features = Vec::new();
    for k in 0..6 {
        features.push(footprint(
            &format!("google:a{k}"),
            30.0002 + k as f64 * 0.0003,
            -1.9995,
        ));
        features.push(footprint(
            &format!("google:b{k}"),
            30.0602 + k as f64 * 0.0003,
            -1.9995,
        ));
        features.push(footprint(
            &format!("google:c{k}"),
            30.1202 + k as f64 * 0.0003,
            -1.9995,
        ));
    }
    Source {
        features,
        version: version.into(),
        cells_asked: 0,
        calls: 0,
    }
}

fn scope(principal: &str) -> Scope {
    Scope {
        principal: principal.into(),
        project: "fixture-project".into(),
    }
}

fn extents(sites: &[&str]) -> Vec<Extent> {
    sites
        .iter()
        .map(|site| {
            let x = match *site {
                "a" => 30.0,
                "b" => 30.06,
                _ => 30.12,
            };
            Extent {
                name: format!("transformer-{site}"),
                bounds: Some([x, -2.0, x + 0.002, -1.998]),
            }
        })
        .collect()
}

struct Machine {
    root: tempfile::TempDir,
    scratch: tempfile::TempDir,
    scope: Scope,
}

impl Machine {
    fn new(principal: &str) -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            scratch: tempfile::tempdir().unwrap(),
            scope: scope(principal),
        }
    }

    fn seed_dataset(
        &self,
        dataset: &Dataset,
        source: &DeclaredSource,
        door: &Door,
        publisher: &mut dyn SourceHost,
        sites: &[&str],
        refresh: bool,
    ) -> Result<SharedOutcome, SharedFailure> {
        let mut door = door.clone();
        let mut shared = SharedSeed {
            revisions: &mut door,
            source: publisher,
            scratch: self.scratch.path(),
            run_id: "seed-fixture",
        };
        seed(
            self.root.path(),
            &self.scope,
            dataset,
            source,
            &buffer_policy(&[200.]).unwrap(),
            &extents(sites),
            &[],
            refresh,
            &mut shared,
        )
    }

    fn seed(
        &self,
        door: &Door,
        publisher: &mut Source,
        sites: &[&str],
        refresh: bool,
    ) -> Result<SharedOutcome, SharedFailure> {
        self.seed_dataset(
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            door,
            publisher,
            sites,
            refresh,
        )
    }

    fn read(&self, sites: &[&str]) -> EnsureOutcome {
        crate::ensure(
            self.root.path(),
            &self.scope,
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &buffer_policy(&[200.]).unwrap(),
            &extents(sites),
            &[],
            Mode::Read,
        )
        .unwrap()
    }

    fn ids(&self, sites: &[&str]) -> Vec<String> {
        let plan = ds_command_kernel::project_dataset_cache::plan(
            &self.scope,
            &buildings_dataset(),
            &buffer_policy(&[200.]).unwrap(),
            &extents(sites),
        )
        .unwrap();
        let mut ids = std::collections::BTreeSet::new();
        for cell in plan.coverage.cells {
            let (features, _) = ensure::read_features(
                self.root.path(),
                &self.scope,
                &buildings_dataset(),
                &polygon(cell),
            )
            .unwrap();
            for feature in features {
                ids.insert(feature["id"].as_str().unwrap().to_owned());
            }
        }
        ids.into_iter().collect()
    }
}

fn manifest(door: &Door) -> Manifest {
    let brain = door.0.borrow();
    chain::decode(
        &Door::manifest_bytes(&brain).unwrap(),
        "fixture-project",
        &buildings_dataset(),
    )
    .unwrap()
}

#[test]
fn the_first_seed_publishes_revision_one_and_a_second_member_reuses_it_without_the_publisher() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    let mut source = publisher("v3");
    let first = a.seed(&door, &mut source, &["a"], false).unwrap();
    assert!(first.ensure.covered && first.ensure.acquired > 0);
    assert!(source.cells_asked > 0);
    assert_eq!(first.published.as_ref().unwrap()["revision"], 1);
    assert_eq!(first.head_revision, Some(1));
    let chain = manifest(&door);
    assert_eq!(chain.revisions.len(), 1);
    assert_eq!(chain.revisions[0].feature_count, 6);
    assert_eq!(chain.revisions[0].source_version, "v3");
    // The seeder's own revision is recorded as applied: a reseed is warm.
    let warm = a.seed(&door, &mut source, &["a"], false).unwrap();
    assert_eq!((warm.ensure.acquired, warm.replayed.len()), (0, 0));
    assert!(warm.published.is_none());
    // A second member on another machine: replay, zero publisher cells.
    let mut untouched = publisher("v3");
    let second = b.seed(&door, &mut untouched, &["a"], false).unwrap();
    assert_eq!(second.replayed, vec![1]);
    assert_eq!((untouched.calls, untouched.cells_asked), (0, 0));
    assert!(second.published.is_none(), "nothing new to publish");
    assert!(second.ensure.covered);
    assert_eq!(b.ids(&["a"]), a.ids(&["a"]));
    assert_eq!(door.0.borrow().published, 1);
}

#[test]
fn extent_growth_acquires_and_publishes_only_the_new_cells() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    let mut source = publisher("v3");
    a.seed(&door, &mut source, &["a"], false).unwrap();
    let asked_before = source.cells_asked;
    let grown = a.seed(&door, &mut source, &["a", "b"], false).unwrap();
    let new_cells = source.cells_asked - asked_before;
    assert!(new_cells > 0);
    assert_eq!(grown.published.as_ref().unwrap()["revision"], 2);
    let chain = manifest(&door);
    assert_eq!(
        chain.revisions[1].feature_count, 6,
        "only site b's footprints"
    );
    let first = chain::coverage(&Manifest {
        revisions: chain.revisions[..1].to_vec(),
        ..chain.clone()
    })
    .unwrap();
    // The second revision's cells never re-publish the first's.
    assert!(
        ds_command_kernel::project_dataset_cache::assess(&first, &chain.revisions[1].coverage)
            .unwrap()
            .gaps
            .cells
            == chain.revisions[1].coverage.cells
    );
    // Member B on the grown extent replays both, still without the publisher.
    let mut untouched = publisher("v3");
    let replayed = b.seed(&door, &mut untouched, &["a", "b"], false).unwrap();
    assert_eq!(replayed.replayed, vec![1, 2]);
    assert_eq!(untouched.cells_asked, 0);
    assert_eq!(b.ids(&["a", "b"]).len(), 12);
    // And a third revision from B reaches A as just that revision.
    let mut source_b = publisher("v3");
    let third = b
        .seed(&door, &mut source_b, &["a", "b", "c"], false)
        .unwrap();
    assert_eq!(third.published.as_ref().unwrap()["revision"], 3);
    let mut none = publisher("v3");
    let pulled = a.seed(&door, &mut none, &["a", "b", "c"], false).unwrap();
    assert_eq!(pulled.replayed, vec![3]);
    assert_eq!(none.cells_asked, 0);
    assert_eq!(a.ids(&["a", "b", "c"]).len(), 18);
}

#[test]
fn a_source_version_change_is_reported_until_an_explicit_refresh() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    a.seed(&door, &mut publisher("v3"), &["a"], false).unwrap();
    // The acquisition owner now reads v4: reported, never acted on.
    let mut v4 = publisher("v4");
    let observed = a.seed(&door, &mut v4, &["a"], false).unwrap();
    assert_eq!(v4.cells_asked, 0);
    assert!(observed.published.is_none());
    assert_eq!(observed.freshness.as_ref().unwrap()["fresh"], false);
    let status = ensure::room_status(a.root.path(), &a.scope, &buildings_dataset()).unwrap();
    assert_eq!(
        status["fresh"], false,
        "the room reports its v3 rows as stale"
    );
    // An individual's explicit refresh re-acquires and republishes.
    let refreshed = a.seed(&door, &mut v4, &["a"], true).unwrap();
    assert!(v4.cells_asked > 0);
    let published = refreshed.published.unwrap();
    assert_eq!(
        (published["revision"].clone(), published["refresh"].clone()),
        (json!(2), json!(true))
    );
    assert_eq!(refreshed.freshness.as_ref().unwrap()["fresh"], true);
    let chain = manifest(&door);
    assert!(chain.revisions[1].refresh);
    assert_eq!(chain.revisions[1].source_version, "v4");
    // Another member picks the refresh up on its next demand-driven seed.
    let mut none = publisher("v4");
    let pulled = b.seed(&door, &mut none, &["a"], false).unwrap();
    assert_eq!(pulled.replayed, vec![1, 2]);
    assert_eq!(none.cells_asked, 0);
    let (features, version) = ensure::read_features(
        b.root.path(),
        &b.scope,
        &buildings_dataset(),
        &polygon([30.0, -2.0, 30.002, -1.998]),
    )
    .unwrap();
    assert_eq!(version, "v4");
    assert!(features.iter().all(|f| f["properties"]["version"] == "v4"));
}

#[test]
fn a_concurrent_publication_is_a_stale_verdict_and_the_replan_never_duplicates() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    // B publishes site a while A is seeding sites a and b.
    let mut source_b = publisher("v3");
    let other = b.seed(&door, &mut source_b, &["a"], false).unwrap();
    let raced = {
        let mut brain = door.0.borrow_mut();
        let revision = brain.outputs.clone();
        // Rewind the door: B's publication becomes the race that lands
        // after A planned against an empty project.
        brain.outputs.clear();
        brain.head_revision = 0;
        brain.published = 0;
        let scratch = tempfile::tempdir().unwrap().keep();
        let outputs = revision
            .iter()
            .map(|(output, bytes)| {
                let path = scratch.join(&output.output_id);
                std::fs::write(&path, bytes).unwrap();
                PublishOutput {
                    output_id: output.output_id.clone(),
                    format: if output.output_id == chain::MANIFEST_OUTPUT {
                        chain::MANIFEST_FORMAT
                    } else {
                        chain::DELTA_FORMAT
                    },
                    content_type: if output.output_id == chain::MANIFEST_OUTPUT {
                        chain::MANIFEST_CONTENT_TYPE
                    } else {
                        chain::DELTA_CONTENT_TYPE
                    },
                    sha256: output.sha256.clone(),
                    size_bytes: output.size_bytes,
                    path,
                }
            })
            .collect();
        Publication {
            head: Head::of("fixture-project", &buildings_dataset()).unwrap(),
            base_fingerprint: chain::base_fingerprint(None),
            client_run_id: "other-member".into(),
            outputs,
        }
    };
    assert!(other.published.is_some());
    door.0.borrow_mut().race = Some(raced);
    let mut source_a = publisher("v3");
    let mine = a.seed(&door, &mut source_a, &["a", "b"], false).unwrap();
    assert_eq!(
        mine.stale_retries, 1,
        "one typed stale verdict, then a re-plan"
    );
    assert_eq!(door.0.borrow().stale, 1);
    assert_eq!(mine.replayed, vec![1], "the re-plan replays what landed");
    let chain = manifest(&door);
    assert_eq!(chain.revisions.len(), 2);
    assert_eq!(chain.revisions[0].feature_count, 6);
    assert_eq!(
        chain.revisions[1].feature_count, 6,
        "only what the race did not cover"
    );
    let first = &chain.revisions[0].coverage;
    let gaps =
        ds_command_kernel::project_dataset_cache::assess(first, &chain.revisions[1].coverage)
            .unwrap();
    assert_eq!(
        gaps.gaps.cells, chain.revisions[1].coverage.cells,
        "no published cell twice"
    );
    assert_eq!(a.ids(&["a", "b"]).len(), 12, "no footprint held twice");
}

#[test]
fn exhausted_replans_stop_with_a_typed_stale_failure_and_keep_held_rows() {
    struct Racing(Door);
    impl RevisionHost for Racing {
        fn read(&mut self, head: &Head) -> Result<Option<Published>, SharedFailure> {
            self.0.read(head)
        }
        fn download(
            &mut self,
            head: &Head,
            output: &Output,
            dest: &Path,
        ) -> Result<(), SharedFailure> {
            self.0.download(head, output, dest)
        }
        fn publish(&mut self, _: &Publication) -> Result<Publish, SharedFailure> {
            Ok(Publish::Stale {
                reason: "someone is always first".into(),
            })
        }
    }
    let machine = Machine::new("user-a");
    let mut racing = Racing(Door(Rc::default()));
    let mut source = publisher("v3");
    let mut shared = SharedSeed {
        revisions: &mut racing,
        source: &mut source,
        scratch: machine.scratch.path(),
        run_id: "seed-race",
    };
    let error = seed(
        machine.root.path(),
        &machine.scope,
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &buffer_policy(&[200.]).unwrap(),
        &extents(&["a"]),
        &[],
        false,
        &mut shared,
    )
    .unwrap_err();
    assert_eq!(error.code(), "project_dataset_revision_stale");
    assert!(machine.read(&["a"]).covered, "held rows stay usable");
}

#[test]
fn a_held_revision_reads_offline_and_an_offline_seed_says_so() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    a.seed(&door, &mut publisher("v3"), &["a"], false).unwrap();
    b.seed(&door, &mut publisher("v3"), &["a"], false).unwrap();
    door.0.borrow_mut().offline = true;
    let reads = door.0.borrow().reads;
    let held = b.read(&["a"]);
    assert!(held.covered && held.features == 6);
    assert_eq!(door.0.borrow().reads, reads, "a read contacts no door");
    let error = b
        .seed(&door, &mut publisher("v3"), &["a"], false)
        .unwrap_err();
    assert_eq!(error.code(), "dataset_provider_unavailable");
    assert!(b.read(&["a"]).covered);
}

#[test]
fn a_tampered_chain_is_refused_whole_and_nothing_is_replayed() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    a.seed(&door, &mut publisher("v3"), &["a"], false).unwrap();
    {
        let mut brain = door.0.borrow_mut();
        let delta = brain
            .outputs
            .iter_mut()
            .find(|(output, _)| output.output_id == "rev-000001")
            .unwrap();
        delta.1[10] ^= 0xff;
    }
    let mut untouched = publisher("v3");
    let error = b.seed(&door, &mut untouched, &["a"], false).unwrap_err();
    assert_eq!(error.code(), "project_dataset_revision_invalid");
    assert_eq!(
        untouched.cells_asked, 0,
        "no silent fallback to the publisher"
    );
    assert!(!b.read(&["a"]).covered);
}

#[test]
fn contours_seed_through_the_same_chain_under_their_own_head() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    let settings = ContourSettings {
        minor_interval_m: 10,
        index_interval_m: 50,
        sample_spacing_m: 10.0,
    };
    let dataset = contours_dataset(settings);
    assert_ne!(
        Head::of("p", &dataset).unwrap(),
        Head::of("p", &buildings_dataset()).unwrap()
    );
    let line = |id: &str, x: f64| {
        json!({"type":"Feature","id":id,"properties":{"elevation":1510.0,"contour_kind":"intermediate"},
            "geometry":{"type":"LineString","coordinates":[[x,-1.9995],[x+0.0005,-1.9993]]}})
    };
    let mut surface = Source {
        features: vec![
            line("contour:1510:a", 30.0003),
            line("contour:1510:b", 30.0603),
        ],
        version: "rwanda_dem_10m/fixture".into(),
        cells_asked: 0,
        calls: 0,
    };
    let source = DeclaredSource::Contours(settings);
    let first = a
        .seed_dataset(&dataset, &source, &door, &mut surface, &["a"], false)
        .unwrap();
    assert_eq!(first.published.as_ref().unwrap()["revision"], 1);
    let mut untouched = Source {
        features: vec![],
        version: "rwanda_dem_10m/fixture".into(),
        cells_asked: 0,
        calls: 0,
    };
    let second = b
        .seed_dataset(&dataset, &source, &door, &mut untouched, &["a"], false)
        .unwrap();
    assert_eq!(
        (second.replayed.clone(), untouched.cells_asked),
        (vec![1], 0)
    );
    assert_eq!(second.ensure.features, 1);
}

/// A surface that reaches only part of the project: cells west of `edge`
/// are answered by the fixture, the rest are declared uncovered.
struct Bordered {
    inner: Source,
    edge: f64,
    asked: usize,
}

impl SourceHost for Bordered {
    fn current_version(&self, dataset: &Dataset) -> Option<String> {
        self.inner.current_version(dataset)
    }
    fn acquire(
        &mut self,
        dataset: &Dataset,
        cells: &[[f64; 4]],
        scratch: &Path,
    ) -> Result<SourceBatch, SharedFailure> {
        self.asked += cells.len();
        let (inside, outside): (Vec<[f64; 4]>, Vec<[f64; 4]>) =
            cells.iter().partition(|cell| cell[2] <= self.edge);
        let mut batch = self.inner.acquire(dataset, &inside, scratch)?;
        batch.uncovered = outside;
        Ok(batch)
    }
}

fn bordered_contours(edge: f64) -> (Dataset, DeclaredSource, Bordered) {
    let settings = ContourSettings {
        minor_interval_m: 10,
        index_interval_m: 50,
        sample_spacing_m: 10.0,
    };
    let line = |id: &str, x: f64| {
        json!({"type":"Feature","id":id,"properties":{"elevation":1510.0,"contour_kind":"intermediate"},
            "geometry":{"type":"LineString","coordinates":[[x,-1.9995],[x+0.0005,-1.9993]]}})
    };
    let surface = Source {
        features: vec![
            line("contour:1510:a", 30.0003),
            line("contour:1510:b", 30.0603),
        ],
        version: "rwanda_dem_10m/fixture".into(),
        cells_asked: 0,
        calls: 0,
    };
    (
        contours_dataset(settings),
        DeclaredSource::Contours(settings),
        Bordered {
            inner: surface,
            edge,
            asked: 0,
        },
    )
}

#[test]
fn covered_contour_cells_publish_and_uncovered_cells_are_itemised_and_asked_again() {
    let door = Door(Rc::default());
    let (a, b) = (Machine::new("user-a"), Machine::new("user-b"));
    // Site a lies on the surface, site b beyond its edge.
    let (dataset, source, mut surface) = bordered_contours(30.05);
    let first = a
        .seed_dataset(&dataset, &source, &door, &mut surface, &["a", "b"], false)
        .unwrap();
    assert!(first.answered, "site a is covered");
    assert!(!first.ensure.covered, "site b stays a gap");
    assert!(!first.ensure.uncovered.is_empty());
    assert!(first.ensure.uncovered.iter().all(|cell| cell[0] >= 30.05));
    assert_eq!(first.ensure.features, 1, "only site a's contour is held");
    let published = first.published.as_ref().expect("the covered cells publish");
    assert_eq!(published["revision"], 1);
    assert_eq!(published["features"], 1);
    let brain = door.0.borrow();
    let chain = chain::decode(
        &Door::manifest_bytes(&brain).unwrap(),
        "fixture-project",
        &dataset,
    )
    .unwrap();
    drop(brain);
    assert!(chain.revisions[0]
        .coverage
        .cells
        .iter()
        .all(|cell| cell[2] <= 30.05));
    assert!(first
        .ensure
        .warnings
        .iter()
        .all(|warning| !warning.contains("Another acquisition")));
    // A reseed asks the surface again for exactly the uncovered cells and
    // publishes nothing new.
    let asked = surface.asked;
    let again = a
        .seed_dataset(&dataset, &source, &door, &mut surface, &["a", "b"], false)
        .unwrap();
    assert_eq!(surface.asked - asked, first.ensure.uncovered.len());
    assert_eq!(again.ensure.uncovered, first.ensure.uncovered);
    assert!(again.answered && again.published.is_none());
    // Another member replays the covered revision without the surface.
    let (_, _, mut untouched) = bordered_contours(30.05);
    let second = b
        .seed_dataset(&dataset, &source, &door, &mut untouched, &["a"], false)
        .unwrap();
    assert_eq!((second.replayed.clone(), untouched.asked), (vec![1], 0));
    assert!(second.ensure.covered && second.ensure.features == 1);
    assert_eq!(door.0.borrow().published, 1);
}

#[test]
fn a_project_the_surface_reaches_nowhere_is_unanswered_and_publishes_nothing() {
    let door = Door(Rc::default());
    let a = Machine::new("user-a");
    let (dataset, source, mut surface) = bordered_contours(29.0);
    let outcome = a
        .seed_dataset(&dataset, &source, &door, &mut surface, &["a", "b"], false)
        .unwrap();
    assert!(!outcome.answered);
    assert!(!outcome.ensure.covered && !outcome.ensure.uncovered.is_empty());
    assert!(outcome.published.is_none());
    assert_eq!((outcome.ensure.features, door.0.borrow().published), (0, 0));
}
