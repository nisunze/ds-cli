use ds_elevation_host::{ComparisonError, FetchEvidence, RwandaSampler};
use ds_geo::projection::GridModelCrs;
use ds_geo_lite::projected_tin::{
    MAX_PROJECTED_TIN_QUERIES, MAX_PROJECTED_TIN_QUERY_TRIANGLE_CHECKS, ProjectedTerrainTin,
};
use ds_terrain_sampling::{MAX_QUERY_CHUNK, MAX_SURFACE_QUERIES, SurfaceEvaluator};

use crate::SamplingFileError;
use crate::input::PreparedSurface;

enum Surface {
    Rwanda(RwandaSampler),
    Survey(ProjectedTerrainTin),
}

pub(crate) struct FileSurface {
    surface: Surface,
    projection: GridModelCrs,
    query_count: usize,
    failure: Option<SamplingFileError>,
}

fn provider_error(error: ComparisonError) -> SamplingFileError {
    match error {
        ComparisonError::Provider(detail) => SamplingFileError::ProviderFailed(detail),
        ComparisonError::Integrity(detail) => SamplingFileError::IntegrityFailed(detail),
        error => SamplingFileError::SurfaceFailed(format!("{}: {error}", error.code())),
    }
}

impl FileSurface {
    pub(crate) fn new(
        surface: PreparedSurface,
        projection: GridModelCrs,
    ) -> Result<Self, SamplingFileError> {
        let surface = match surface {
            PreparedSurface::Rwanda => {
                Surface::Rwanda(RwandaSampler::new().map_err(provider_error)?)
            }
            PreparedSurface::Survey(tin) => Surface::Survey(tin),
        };
        Ok(Self {
            surface,
            projection,
            query_count: 0,
            failure: None,
        })
    }

    pub(crate) fn take_failure(&mut self) -> Option<SamplingFileError> {
        self.failure.take()
    }

    pub(crate) fn acquisition_evidence(&self) -> &[FetchEvidence] {
        match &self.surface {
            Surface::Rwanda(sampler) => sampler.acquisition_evidence(),
            Surface::Survey(_) => &[],
        }
    }

    fn evaluate(&mut self, points: &[[f64; 2]]) -> Result<Vec<Option<f64>>, SamplingFileError> {
        if points.len() > MAX_QUERY_CHUNK {
            return Err(SamplingFileError::InputTooLarge(
                "surface batch exceeds 20000 queries".into(),
            ));
        }
        self.query_count = self
            .query_count
            .checked_add(points.len())
            .filter(|count| *count <= MAX_SURFACE_QUERIES)
            .ok_or_else(|| {
                SamplingFileError::InputTooLarge("aggregate surface query budget exceeded".into())
            })?;
        match &mut self.surface {
            Surface::Rwanda(sampler) => {
                let wgs84: Vec<_> = points
                    .iter()
                    .map(|point| self.projection.to_map(*point))
                    .collect::<Result<_, _>>()
                    .map_err(|error| SamplingFileError::SurfaceFailed(error.to_string()))?;
                sampler.sample_wgs84(&wgs84).map_err(provider_error)
            }
            Surface::Survey(tin) => {
                // A conservative batch size bounds candidate checks even in
                // the densest spatial cell. Chunking cannot change query XY.
                let chunk_size = (MAX_PROJECTED_TIN_QUERY_TRIANGLE_CHECKS
                    / tin.triangles().len().max(1))
                .clamp(1, MAX_PROJECTED_TIN_QUERIES);
                let mut elevations = Vec::with_capacity(points.len());
                for chunk in points.chunks(chunk_size) {
                    elevations
                        .extend(tin.sample_many(chunk).map_err(|error| {
                            SamplingFileError::SurfaceFailed(error.to_string())
                        })?);
                }
                Ok(elevations)
            }
        }
    }
}

impl SurfaceEvaluator for FileSurface {
    fn elevations(&mut self, points_xy_m: &[[f64; 2]]) -> Result<Vec<Option<f64>>, String> {
        match self.evaluate(points_xy_m) {
            Ok(values) => Ok(values),
            Err(error) => {
                let detail = error.to_string();
                self.failure = Some(error);
                Err(detail)
            }
        }
    }
}
