//! Internal backend-neutral outline-to-MTSDF generation for GPUI renderers.

use anyhow::{Result, ensure};
use bymsdfgen_core::{
    Bitmap, Contour, DistanceMapping, EdgeSegment, ErrorCorrectionMode, FillRule,
    MsdfGeneratorConfig, Projection, Range, SdfTransformation, Shape, Vector2,
    coloring::edge_coloring_simple,
    correction::msdf_error_correction,
    generator::{DistanceCheckMode, generate_mtsdf},
    raster::distance_sign_correction_multi,
};
use gpui::{Bounds, DevicePixels, MsdfGlyphInfo, MsdfGlyphParams, point, size};

/// Unscaled native-font bounds in the same y-up coordinate system as the outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// A native outline with its exact font-instance bounds and normalization.
pub struct GlyphOutline {
    shape: Shape,
    bounds: OutlineBounds,
    units_per_em: f64,
}

/// Backend-neutral collector for line, quadratic, and cubic outline commands.
#[derive(Default)]
pub struct OutlineBuilder {
    shape: Shape,
    contour: Option<Contour>,
    start: Option<Vector2>,
    current: Option<Vector2>,
}

impl OutlineBuilder {
    pub fn move_to(&mut self, x: f64, y: f64) {
        self.finish_contour();
        let point = Vector2::new(x, y);
        self.contour = Some(Contour::new());
        self.start = Some(point);
        self.current = Some(point);
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        let to = Vector2::new(x, y);
        if let (Some(contour), Some(from)) = (&mut self.contour, self.current) {
            contour.add_edge(EdgeSegment::line(from, to));
            self.current = Some(to);
        }
    }

    pub fn quadratic_to(&mut self, control_x: f64, control_y: f64, x: f64, y: f64) {
        let to = Vector2::new(x, y);
        if let (Some(contour), Some(from)) = (&mut self.contour, self.current) {
            contour.add_edge(EdgeSegment::quadratic(
                from,
                Vector2::new(control_x, control_y),
                to,
            ));
            self.current = Some(to);
        }
    }

    pub fn cubic_to(
        &mut self,
        control_1_x: f64,
        control_1_y: f64,
        control_2_x: f64,
        control_2_y: f64,
        x: f64,
        y: f64,
    ) {
        let to = Vector2::new(x, y);
        if let (Some(contour), Some(from)) = (&mut self.contour, self.current) {
            contour.add_edge(EdgeSegment::cubic(
                from,
                Vector2::new(control_1_x, control_1_y),
                Vector2::new(control_2_x, control_2_y),
                to,
            ));
            self.current = Some(to);
        }
    }

    pub fn close(&mut self) {
        self.finish_contour();
    }

    /// Finalize without changing native contour winding or overlap semantics.
    pub fn finish(mut self, bounds: OutlineBounds, units_per_em: f64) -> Option<GlyphOutline> {
        self.finish_contour();
        if units_per_em <= 0.0
            || bounds.min_x >= bounds.max_x
            || bounds.min_y >= bounds.max_y
            || self.shape.edge_count() == 0
            || !self.shape.validate()
        {
            return None;
        }
        self.shape.normalize();
        edge_coloring_simple(&mut self.shape, 3.0, 0);
        self.shape.validate().then_some(GlyphOutline {
            shape: self.shape,
            bounds,
            units_per_em,
        })
    }

    fn finish_contour(&mut self) {
        let (Some(mut contour), Some(start), Some(current)) =
            (self.contour.take(), self.start.take(), self.current.take())
        else {
            return;
        };
        if current != start {
            contour.add_edge(EdgeSegment::line(current, start));
        }
        if !contour.is_empty() {
            self.shape.add_contour(contour);
        }
    }
}

impl GlyphOutline {
    pub fn glyph_info(&self, params: &MsdfGlyphParams) -> Result<MsdfGlyphInfo> {
        let region = self.region(params);
        let padding_x_em = self.width_em() as f32 * (region.padding_x as f32 - 0.5).max(0.0)
            / region.inner_width as f32;
        let padding_y_em = self.height_em() as f32 * (region.padding_y as f32 - 0.5).max(0.0)
            / region.inner_height as f32;

        Ok(MsdfGlyphInfo {
            bounds_em: Bounds {
                origin: point(
                    self.bounds.min_x as f32 / self.units_per_em as f32 - padding_x_em,
                    -(self.bounds.max_y as f32 / self.units_per_em as f32) - padding_y_em,
                ),
                size: size(
                    self.width_em() as f32 + 2.0 * padding_x_em,
                    self.height_em() as f32 + 2.0 * padding_y_em,
                ),
            },
            raster_size: size(
                DevicePixels(region.total_width().try_into()?),
                DevicePixels(region.total_height().try_into()?),
            ),
            field_padding_em: padding_x_em.min(padding_y_em),
        })
    }

    pub fn rasterize(
        &self,
        params: &MsdfGlyphParams,
        expected_info: MsdfGlyphInfo,
    ) -> Result<Option<Vec<u8>>> {
        let region = self.region(params);
        ensure!(
            self.glyph_info(params)?.raster_size == expected_info.raster_size,
            "MTSDF geometry changed between bounds and atlas generation"
        );

        let shape_width = self.bounds.max_x - self.bounds.min_x;
        let shape_height = self.bounds.max_y - self.bounds.min_y;
        let scale = Vector2::new(
            region.inner_width as f64 / shape_width,
            region.inner_height as f64 / shape_height,
        );
        let translation = Vector2::new(
            region.padding_x as f64 / scale.x - self.bounds.min_x,
            region.padding_y as f64 / scale.y - self.bounds.min_y,
        );
        let transformation = SdfTransformation::new(
            Projection::new(scale, translation),
            DistanceMapping::from_range(Range::symmetric(self.units_per_em)),
        );

        let mut config = MsdfGeneratorConfig::default();
        config.error_correction.mode = ErrorCorrectionMode::Disabled;
        let mut bitmap: Bitmap<f32, 4> = Bitmap::new(region.total_width(), region.total_height());
        generate_mtsdf(&mut bitmap, &self.shape, &transformation, &config);
        distance_sign_correction_multi(
            &mut bitmap,
            &self.shape,
            &transformation.projection,
            0.5,
            FillRule::NonZero,
        );
        config.error_correction.mode = ErrorCorrectionMode::EdgePriority;
        config.error_correction.distance_check_mode = DistanceCheckMode::AlwaysCheckDistance;
        msdf_error_correction(&mut bitmap, &self.shape, &transformation, &config);

        let mut bytes = Vec::with_capacity(region.total_width() * region.total_height() * 4);
        for y in (0..region.total_height()).rev() {
            for x in 0..region.total_width() {
                bytes.extend(
                    bitmap
                        .pixel(x, y)
                        .iter()
                        .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8),
                );
            }
        }
        Ok(
            has_closed_exterior(&bytes, region.total_width(), region.total_height())
                .then_some(bytes),
        )
    }

    fn width_em(&self) -> f64 {
        (self.bounds.max_x - self.bounds.min_x) / self.units_per_em
    }

    fn height_em(&self) -> f64 {
        (self.bounds.max_y - self.bounds.min_y) / self.units_per_em
    }

    fn region(&self, params: &MsdfGlyphParams) -> Region {
        let pixels_per_em = f64::from(params.generation_em_pixels);
        Region {
            inner_width: (self.width_em() * pixels_per_em).ceil().max(1.0) as usize,
            inner_height: (self.height_em() * pixels_per_em).ceil().max(1.0) as usize,
            padding_x: usize::from(params.padding_pixels),
            padding_y: usize::from(params.padding_pixels),
        }
    }
}

#[derive(Clone, Copy)]
struct Region {
    inner_width: usize,
    inner_height: usize,
    padding_x: usize,
    padding_y: usize,
}

impl Region {
    fn total_width(self) -> usize {
        self.inner_width + 2 * self.padding_x
    }

    fn total_height(self) -> usize {
        self.inner_height + 2 * self.padding_y
    }
}

fn has_closed_exterior(bytes: &[u8], width: usize, height: usize) -> bool {
    if width < 2 || height < 2 || bytes.len() != width * height * 4 {
        return false;
    }
    let is_outside = |x: usize, y: usize| {
        let pixel = &bytes[(y * width + x) * 4..][..4];
        let mut rgb = [pixel[0], pixel[1], pixel[2]];
        rgb.sort_unstable();
        rgb[1] < 128 && pixel[3] < 128
    };
    (0..width).all(|x| is_outside(x, 0) && is_outside(x, height - 1))
        && (1..height - 1).all(|y| is_outside(0, y) && is_outside(width - 1, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{FontId, GlyphId};

    fn overlapping_cross() -> GlyphOutline {
        let mut outline = OutlineBuilder::default();
        // Both contours intentionally have the same native winding and overlap. Reorienting them
        // as if one were a nested hole creates the exact lowercase-t artifact this guards against.
        outline.move_to(0.0, 60.0);
        outline.line_to(100.0, 60.0);
        outline.line_to(100.0, 40.0);
        outline.line_to(0.0, 40.0);
        outline.close();
        outline.move_to(40.0, 100.0);
        outline.line_to(60.0, 100.0);
        outline.line_to(60.0, 0.0);
        outline.line_to(40.0, 0.0);
        outline.close();
        outline
            .finish(
                OutlineBounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 100.0,
                    max_y: 100.0,
                },
                100.0,
            )
            .unwrap()
    }

    fn overlapping_w_join() -> GlyphOutline {
        let mut outline = OutlineBuilder::default();
        // Two same-winding diagonal strokes overlap at the lower join, as happens in fonts that
        // retain component overlap in a lowercase w.
        outline.move_to(0.0, 100.0);
        outline.line_to(18.0, 100.0);
        outline.line_to(60.0, 0.0);
        outline.line_to(42.0, 0.0);
        outline.close();
        outline.move_to(82.0, 100.0);
        outline.line_to(100.0, 100.0);
        outline.line_to(58.0, 0.0);
        outline.line_to(40.0, 0.0);
        outline.close();
        outline
            .finish(
                OutlineBounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 100.0,
                    max_y: 100.0,
                },
                100.0,
            )
            .unwrap()
    }

    fn median_at(bytes: &[u8], width: usize, x: usize, y: usize) -> u8 {
        let mut rgb = [
            bytes[(y * width + x) * 4],
            bytes[(y * width + x) * 4 + 1],
            bytes[(y * width + x) * 4 + 2],
        ];
        rgb.sort_unstable();
        rgb[1]
    }

    #[test]
    fn overlapping_contours_keep_their_filled_intersection() -> Result<()> {
        let outline = overlapping_cross();
        let params = MsdfGlyphParams::new(FontId(7), GlyphId(9));
        let info = outline.glyph_info(&params)?;
        let bytes = outline.rasterize(&params, info)?.unwrap();
        let x = (info.raster_size.width.0 / 2) as usize;
        let y = (info.raster_size.height.0 / 2) as usize;
        let width = info.raster_size.width.0 as usize;
        assert!(
            median_at(&bytes, width, x, y) > 128,
            "overlapping native contours became a hole"
        );
        Ok(())
    }

    #[test]
    fn overlapping_w_join_stays_filled() -> Result<()> {
        let outline = overlapping_w_join();
        let params = MsdfGlyphParams::new(FontId(11), GlyphId(13));
        let info = outline.glyph_info(&params)?;
        let bytes = outline.rasterize(&params, info)?.unwrap();
        let width = info.raster_size.width.0 as usize;
        let height = info.raster_size.height.0 as usize;
        assert!(
            median_at(&bytes, width, width / 2, height * 4 / 5) > 128,
            "overlapping lowercase-w join became a hole"
        );
        Ok(())
    }

    #[test]
    fn fixed_params_produce_stable_geometry_and_bytes() -> Result<()> {
        let outline = overlapping_cross();
        let params = MsdfGlyphParams::new(FontId(1), GlyphId(2));
        let info = outline.glyph_info(&params)?;
        let first = outline.rasterize(&params, info)?.unwrap();
        let second = outline.rasterize(&params, info)?.unwrap();
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn rejects_a_zero_contour_reaching_the_tile_edge() {
        let width = 3;
        let height = 3;
        let mut valid = vec![0; width * height * 4];
        valid[(width + 1) * 4..(width + 1) * 4 + 4].copy_from_slice(&[255; 4]);
        assert!(has_closed_exterior(&valid, width, height));

        let mut leaking = valid;
        leaking[(width + 2) * 4 + 3] = 128;
        assert!(!has_closed_exterior(&leaking, width, height));
    }
}
