use super::canvas_texture_from_preview;
#[cfg(test)]
use crate::canvas::CanvasImagePreview;
use crate::canvas::geometry::merge_cropped_item_fields;
use crate::canvas::{CanvasCropRequest, CanvasState, SharedRgba};
use crate::data::{CanvasImageLod, CanvasStackItem, Placement};
use crate::image_preparation::{PreparedCropPreview, PreparedPlacementImage};
use eframe::egui;
use std::sync::Arc;
/// A borrowed draw view. Placement values remain in the document; resources are keyed by identity.
pub(in crate::canvas) struct RenderedPlacement<'a> {
    pub(crate) placement: &'a Placement,
    image: &'a PlacementImage,
}
impl std::ops::Deref for RenderedPlacement<'_> {
    type Target = PlacementImage;
    fn deref(&self) -> &Self::Target {
        self.image
    }
}
#[derive(Clone)]
pub(in crate::canvas) struct PendingCropPreview {
    pub(in crate::canvas) placement: Placement,
    pub(in crate::canvas) image: PlacementImage,
    pub(in crate::canvas) rgba: SharedRgba,
}

#[derive(Clone)]
pub(in crate::canvas) enum CanvasTexture {
    Managed(egui::TextureHandle),
    Native(Arc<crate::image_preparation::NativeTextureHandle>),
    Svg {
        uri: String,
        bytes: Arc<[u8]>,
        size: [usize; 2],
    },
    Unloaded {
        size: [usize; 2],
    },
}

impl CanvasTexture {
    pub(in crate::canvas) fn id(&self) -> egui::TextureId {
        match self {
            Self::Managed(texture) => texture.id(),
            Self::Native(texture) => texture.texture_id(),
            Self::Svg { .. } => egui::TextureId::Managed(0),
            Self::Unloaded { .. } => egui::TextureId::Managed(0),
        }
    }

    pub(in crate::canvas) fn size(&self) -> [usize; 2] {
        match self {
            Self::Managed(texture) => texture.size(),
            Self::Native(texture) => texture.size(),
            Self::Svg { size, .. } => *size,
            Self::Unloaded { size } => *size,
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::canvas) struct AnimatedCanvasFrame {
    pub(in crate::canvas) rgba: SharedRgba,
    pub(in crate::canvas) delay_seconds: f32,
}

#[derive(Clone, Debug)]
pub(in crate::canvas) struct AnimatedCanvasImage {
    pub(in crate::canvas) frames: Vec<AnimatedCanvasFrame>,
    pub(in crate::canvas) width: u32,
    pub(in crate::canvas) height: u32,
    pub(in crate::canvas) current_frame: usize,
}

#[derive(Clone)]
pub(in crate::canvas) struct PlacementImage {
    pub(in crate::canvas) texture: CanvasTexture,
    pub(in crate::canvas) loaded_lod: CanvasImageLod,
    pub(in crate::canvas) animation: Option<AnimatedCanvasImage>,
}
struct CachedCropPreview {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

fn crop_cached_preview(
    rgba: &[u8],
    preview_width: u32,
    preview_height: u32,
    asset_width: u32,
    asset_height: u32,
    crop_rect: crate::canvas::PixelCropRect,
) -> Result<CachedCropPreview, String> {
    if preview_width == 0 || preview_height == 0 || asset_width == 0 || asset_height == 0 {
        return Err("cannot crop zero-sized preview".to_owned());
    }
    let expected_len = preview_width as usize * preview_height as usize * 4;
    if rgba.len() != expected_len {
        return Err("cached preview pixels did not match preview dimensions".to_owned());
    }

    let x0 = scale_floor(crop_rect.x, preview_width, asset_width).min(preview_width - 1);
    let y0 = scale_floor(crop_rect.y, preview_height, asset_height).min(preview_height - 1);
    let x1 = scale_ceil(
        crop_rect.x.saturating_add(crop_rect.width),
        preview_width,
        asset_width,
    )
    .clamp(x0 + 1, preview_width);
    let y1 = scale_ceil(
        crop_rect.y.saturating_add(crop_rect.height),
        preview_height,
        asset_height,
    )
    .clamp(y0 + 1, preview_height);
    let width = x1 - x0;
    let height = y1 - y0;
    let mut cropped = Vec::with_capacity(width as usize * height as usize * 4);
    for y in y0..y1 {
        let row_start = (y as usize * preview_width as usize + x0 as usize) * 4;
        let row_end = row_start + width as usize * 4;
        cropped.extend_from_slice(&rgba[row_start..row_end]);
    }

    Ok(CachedCropPreview {
        rgba: cropped,
        width,
        height,
    })
}

fn scale_floor(value: u32, target: u32, source: u32) -> u32 {
    ((value as u64 * target as u64) / source as u64) as u32
}

fn scale_ceil(value: u32, target: u32, source: u32) -> u32 {
    let numerator = value as u64 * target as u64;
    let denominator = source as u64;
    numerator.div_ceil(denominator) as u32
}

impl CanvasState {
    pub fn add_prepared_placement_image(
        &mut self,
        ctx: &egui::Context,
        loaded: PreparedPlacementImage,
    ) {
        let animation = loaded.animated_preview.map(|animated| AnimatedCanvasImage {
            frames: animated
                .frames
                .into_iter()
                .map(|frame| AnimatedCanvasFrame {
                    rgba: SharedRgba::from(frame.rgba),
                    delay_seconds: frame.delay_seconds,
                })
                .collect(),
            width: animated.width,
            height: animated.height,
            current_frame: 0,
        });
        let texture = canvas_texture_from_preview(
            ctx,
            format!(
                "asset-{}-item-{}",
                loaded.placement.require_loaded_asset().id,
                loaded.placement.state.id
            ),
            &loaded.preview_rgba,
            loaded.preview_width,
            loaded.preview_height,
            loaded.preview_texture,
        );

        self.install_placement_image(
            loaded.placement,
            PlacementImage {
                texture,
                loaded_lod: loaded.loaded_lod,
                animation,
            },
        );
    }
    pub(in crate::canvas) fn rendered_placement_by_id(
        &self,
        id: i64,
    ) -> Option<RenderedPlacement<'_>> {
        Some(RenderedPlacement {
            placement: self.placement_by_id(id)?,
            image: self.render.placement_images.get(&id)?,
        })
    }
    pub(in crate::canvas) fn install_placement_image(
        &mut self,
        placement: Placement,
        image: PlacementImage,
    ) {
        let id = placement.id();
        self.document.placements.retain(|p| p.id() != id);
        self.document.placements.push(placement);
        self.document
            .placements
            .sort_by_key(|p| p.state.geometry.z_index);
        self.render.placement_images.insert(id, image);
        self.render.image_lod_loads.remove(&id);
        self.append_missing_stack_item(CanvasStackItem::Placement(id));
    }
    pub fn add_unresolved_placement(&mut self, placement: Placement) {
        self.install_placement_image(
            placement.clone(),
            PlacementImage {
                texture: CanvasTexture::Unloaded {
                    size: [
                        placement.state.geometry.size.x.max(1.0).round() as usize,
                        placement.state.geometry.size.y.max(1.0).round() as usize,
                    ],
                },
                loaded_lod: crate::data::CanvasImageLod::Overview,
                animation: None,
            },
        );
    }
    pub fn add_loaded_reference_placement(&mut self, placement: Placement) -> Result<(), String> {
        let source = self
            .document
            .placements
            .iter()
            .find(|p| p.asset_id() == placement.asset_id())
            .and_then(|p| self.render.placement_images.get(&p.id()))
            .ok_or_else(|| {
                format!(
                    "missing source texture for asset {}",
                    placement.require_loaded_asset().id
                )
            })?;
        let texture = source.texture.clone();
        let animation = source.animation.clone();

        self.install_placement_image(
            placement.clone(),
            PlacementImage {
                texture,
                loaded_lod: CanvasImageLod::Working,
                animation,
            },
        );
        Ok(())
    }
    pub fn warm_cached_pixels_for_item(
        &mut self,
        library: &crate::library::storage::Library,
        placement_id: i64,
    ) -> Result<(), String> {
        let image = self
            .rendered_placement_by_id(placement_id)
            .ok_or_else(|| format!("missing canvas item {placement_id}"))?;
        let [width, height] = image.texture.size();
        let expected_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "canvas pixel cache dimensions overflowed".to_owned())?;
        if self
            .render
            .asset_rgba_cache
            .get(&image.placement.require_loaded_asset().id)
            .is_some_and(|rgba| rgba.len() == expected_len)
        {
            return Ok(());
        }

        let placement = image.placement.clone();
        let (rgba, loaded_width, loaded_height) =
            crate::image_preparation::load_canvas_snapshot_pixels(
                library,
                &placement,
                width as u32,
                height as u32,
            )?;
        if loaded_width as usize != width || loaded_height as usize != height {
            return Err("cached canvas pixels did not match texture size".to_owned());
        }
        self.render
            .asset_rgba_cache
            .insert(placement.require_loaded_asset().id, SharedRgba::from(rgba));
        Ok(())
    }
    pub fn snapshot_item_image(
        &self,
        placement_id: i64,
    ) -> Result<(Placement, SharedRgba, u32, u32), String> {
        if let Some(preview) = self.render.pending_crop_previews.get(&placement_id) {
            let [width, height] = preview.image.texture.size();
            return Ok((
                preview.placement.clone(),
                preview.rgba.clone(),
                width as u32,
                height as u32,
            ));
        }

        let image = self
            .rendered_placement_by_id(placement_id)
            .ok_or_else(|| format!("missing canvas item {}", placement_id))?;
        let rgba = self
            .render
            .asset_rgba_cache
            .get(&image.placement.require_loaded_asset().id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing cached pixels for asset {}",
                    image.placement.require_loaded_asset().id
                )
            })?;
        Ok((
            image.placement.clone(),
            rgba,
            image.texture.size()[0] as u32,
            image.texture.size()[1] as u32,
        ))
    }
    pub fn projected_cropped_item(
        &self,
        cropped: &PreparedCropPreview,
    ) -> Result<crate::data::PlacementState, String> {
        let source_placement_state =
            if let Some(preview) = self.render.pending_crop_previews.get(&cropped.placement_id) {
                preview.placement.state.clone()
            } else {
                let Some(image) = self.rendered_placement_by_id(cropped.placement_id) else {
                    return Err(format!("missing canvas item {}", cropped.placement_id));
                };
                image.placement.state.clone()
            };

        let mut updated_placement_state = source_placement_state;
        merge_cropped_item_fields(&mut updated_placement_state, cropped.crop_offset_local);
        Ok(updated_placement_state)
    }
    pub fn apply_pending_crop_preview(
        &mut self,
        ctx: &egui::Context,
        request: &CanvasCropRequest,
    ) -> Result<(), String> {
        let image_index = self
            .document
            .placements
            .iter()
            .position(|image| image.state.id == request.placement_id)
            .ok_or_else(|| format!("missing canvas item {}", request.placement_id))?;
        let original_placement = self.document.placements[image_index].clone();
        let original_image = self
            .render
            .placement_images
            .get(&request.placement_id)
            .cloned()
            .ok_or_else(|| "missing crop image".to_owned())?;
        let source_rgba = self
            .render
            .asset_rgba_cache
            .get(&request.source_asset.id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing cached pixels for asset {}",
                    request.source_asset.id
                )
            })?;
        self.render
            .pending_crop_previews
            .entry(request.placement_id)
            .or_insert_with(|| PendingCropPreview {
                placement: original_placement.clone(),
                image: original_image.clone(),
                rgba: source_rgba.clone(),
            });

        let [source_width, source_height] = original_image.texture.size();
        let preview = crop_cached_preview(
            &source_rgba,
            source_width as u32,
            source_height as u32,
            request.source_asset.width,
            request.source_asset.height,
            request.crop_rect,
        )?;
        let texture = canvas_texture_from_preview(
            ctx,
            format!(
                "asset-{}-item-{}-pending-crop",
                request.source_asset.id, request.placement_id
            ),
            &preview.rgba,
            preview.width,
            preview.height,
            crate::image_preparation::ImagePrepPreviewTexture::Cpu,
        );
        let mut asset = request.source_asset.clone();
        asset.width = request.crop_rect.width;
        asset.height = request.crop_rect.height;
        let mut updated_placement_state = original_placement.state.clone();
        merge_cropped_item_fields(&mut updated_placement_state, request.crop_offset_local);

        self.document.placements[image_index].state = updated_placement_state;
        self.document.placements[image_index].content =
            crate::data::PlacementContent::Loaded { asset };
        let image = self
            .render
            .placement_images
            .get_mut(&request.placement_id)
            .expect("crop image exists");
        image.texture = texture;
        image.loaded_lod = CanvasImageLod::Working;
        image.animation = None;

        Ok(())
    }
    pub fn apply_cropped_item(
        &mut self,
        ctx: &egui::Context,
        cropped: PreparedCropPreview,
        asset: crate::data::Asset,
        updated_item: crate::data::PlacementState,
    ) -> Result<(), String> {
        let placement_id = cropped.placement_id;
        let loaded_lod = cropped.loaded_lod;
        let should_cache_pixels = self
            .interaction
            .selected_placement_ids
            .contains(&placement_id);
        let texture = canvas_texture_from_preview(
            ctx,
            format!("asset-{}-item-{}", asset.id, placement_id),
            &cropped.preview_rgba,
            cropped.preview_width,
            cropped.preview_height,
            cropped.preview_texture,
        );
        if should_cache_pixels {
            self.render
                .asset_rgba_cache
                .insert(asset.id, SharedRgba::from(cropped.preview_rgba));
        }

        let Some(image) = self
            .document
            .placements
            .iter_mut()
            .find(|image| image.state.id == placement_id)
        else {
            return Err(format!("missing canvas item {}", placement_id));
        };

        image.state = updated_item;
        image.content = crate::data::PlacementContent::Loaded { asset };
        let image = self
            .render
            .placement_images
            .get_mut(&placement_id)
            .ok_or_else(|| "missing crop image".to_owned())?;
        image.texture = texture;
        image.loaded_lod = loaded_lod;
        image.animation = None;
        self.render.image_lod_loads.remove(&placement_id);
        self.render.pending_crop_previews.remove(&placement_id);
        self.mark_placement_for_persistence(placement_id);
        Ok(())
    }
    #[cfg(test)]
    pub fn preview_for_placement_id(&self, placement_id: i64) -> Option<CanvasImagePreview> {
        let image = self.rendered_placement_by_id(placement_id)?;
        let [width, height] = image.texture.size();
        Some(CanvasImagePreview {
            texture_size: egui::vec2(width as f32, height as f32),
        })
    }
    pub fn undispatched_image_lod_loads(&mut self) -> Vec<(i64, CanvasImageLod)> {
        self.render
            .image_lod_loads
            .iter_mut()
            .filter(|(_, load)| !load.dispatched)
            .map(|(placement_id, load)| {
                load.dispatched = true;
                (*placement_id, load.lod)
            })
            .collect()
    }
    pub fn note_image_lod_load_dispatched(&mut self, placement_id: i64, lod: CanvasImageLod) {
        self.render.image_lod_loads.insert(
            placement_id,
            crate::canvas::state::ImageLodLoad {
                lod,
                dispatched: true,
            },
        );
    }
    pub fn has_pending_image_loads(&self) -> bool {
        !self.render.image_lod_loads.is_empty()
    }
    pub fn clear_crop_pending(&mut self, placement_id: i64) {
        if let Some(preview) = self.render.pending_crop_previews.remove(&placement_id)
            && let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|image| image.state.id == placement_id)
        {
            *image = preview.placement;
            self.render
                .placement_images
                .insert(placement_id, preview.image);
        }
        self.interaction
            .crop_pending_placement_ids
            .retain(|pending_id| *pending_id != placement_id);
    }
    pub fn clear_image_lod_load(&mut self, placement_id: i64) {
        self.render.image_lod_loads.remove(&placement_id);
    }
    pub fn prune_cached_pixels_to_selected_items(&mut self) {
        let selected_asset_ids = self
            .document
            .placements
            .iter()
            .filter(|image| {
                self.interaction
                    .selected_placement_ids
                    .contains(&image.state.id)
            })
            .filter_map(|image| image.asset_id())
            .collect::<Vec<_>>();
        self.render
            .asset_rgba_cache
            .retain(|asset_id, _| selected_asset_ids.contains(asset_id));
    }
    pub(super) fn request_image_lod_load(&mut self, placement_id: i64, lod: CanvasImageLod) {
        let Some(image) = self.rendered_placement_by_id(placement_id) else {
            return;
        };
        if lod <= image.loaded_lod {
            return;
        }
        match self.render.image_lod_loads.get(&placement_id) {
            Some(load) if load.lod >= lod => {}
            _ => {
                self.render.image_lod_loads.insert(
                    placement_id,
                    crate::canvas::state::ImageLodLoad {
                        lod,
                        dispatched: false,
                    },
                );
            }
        }
    }
    pub(in crate::canvas) fn advance_animated_image(
        &mut self,
        placement_id: i64,
        time_seconds: f32,
    ) -> Option<(SharedRgba, u32, u32)> {
        let image = self.render.placement_images.get_mut(&placement_id)?;
        let animation = image.animation.as_mut()?;
        if animation.frames.is_empty() {
            return None;
        }

        let total_duration = animation
            .frames
            .iter()
            .map(|frame| frame.delay_seconds.max(0.01))
            .sum::<f32>()
            .max(0.01);
        let mut remaining = time_seconds.rem_euclid(total_duration);
        let mut frame_index = 0;
        for (index, frame) in animation.frames.iter().enumerate() {
            let delay = frame.delay_seconds.max(0.01);
            if remaining <= delay {
                frame_index = index;
                break;
            }
            remaining -= delay;
            frame_index = index;
        }

        if animation.current_frame == frame_index {
            return None;
        }

        animation.current_frame = frame_index;
        let frame = &animation.frames[frame_index];
        Some((frame.rgba.clone(), animation.width, animation.height))
    }
    pub(super) fn animated_image_repaint_delay(
        &self,
        placement_id: i64,
        time_seconds: f32,
    ) -> Option<std::time::Duration> {
        let image = self.rendered_placement_by_id(placement_id)?;
        let animation = image.animation.as_ref()?;
        if animation.frames.is_empty() {
            return None;
        }

        let total_duration = animation
            .frames
            .iter()
            .map(|frame| frame.delay_seconds.max(0.01))
            .sum::<f32>()
            .max(0.01);
        let mut remaining = time_seconds.rem_euclid(total_duration);
        for frame in &animation.frames {
            let delay = frame.delay_seconds.max(0.01);
            if remaining <= delay {
                return Some(std::time::Duration::from_secs_f32(
                    (delay - remaining).max(0.01),
                ));
            }
            remaining -= delay;
        }

        Some(std::time::Duration::from_secs_f32(0.01))
    }
}
