use std::fmt;
use std::sync::Arc;

use eframe::egui;

#[derive(Clone)]
pub enum ImportedPreviewTexture {
    Cpu,
    Native(Arc<NativeTextureHandle>),
    Svg {
        uri: String,
        bytes: Arc<[u8]>,
        size: [usize; 2],
    },
}

impl fmt::Debug for ImportedPreviewTexture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("Cpu"),
            Self::Native(handle) => f.debug_tuple("Native").field(handle).finish(),
            Self::Svg { uri, size, .. } => f
                .debug_struct("Svg")
                .field("uri", uri)
                .field("size", size)
                .finish(),
        }
    }
}

pub struct NativeTextureHandle {
    texture_id: egui::TextureId,
    size: [usize; 2],
    cleanup: Arc<dyn Fn(egui::TextureId) + Send + Sync>,
    _texture: Option<eframe::wgpu::Texture>,
    view: Option<Arc<eframe::wgpu::TextureView>>,
}

impl fmt::Debug for NativeTextureHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeTextureHandle")
            .field("texture_id", &self.texture_id)
            .field("size", &self.size)
            .finish()
    }
}

impl NativeTextureHandle {
    fn from_registered_texture(
        render_state: eframe::egui_wgpu::RenderState,
        texture_id: egui::TextureId,
        width: u32,
        height: u32,
        texture: eframe::wgpu::Texture,
        view: eframe::wgpu::TextureView,
    ) -> Self {
        let cleanup = Arc::new(move |texture_id| {
            render_state.renderer.write().free_texture(&texture_id);
        });
        Self {
            texture_id,
            size: [width as usize, height as usize],
            cleanup,
            _texture: Some(texture),
            view: Some(Arc::new(view)),
        }
    }

    pub fn texture_id(&self) -> egui::TextureId {
        self.texture_id
    }

    pub fn size(&self) -> [usize; 2] {
        self.size
    }

    pub fn create_bind_group(
        &self,
        device: &eframe::wgpu::Device,
        layout: &eframe::wgpu::BindGroupLayout,
        sampler: &eframe::wgpu::Sampler,
    ) -> Option<eframe::wgpu::BindGroup> {
        let view = self.view.as_ref()?;
        Some(
            device.create_bind_group(&eframe::wgpu::BindGroupDescriptor {
                label: Some("snappyref_canvas_image_bind_group"),
                layout,
                entries: &[
                    eframe::wgpu::BindGroupEntry {
                        binding: 0,
                        resource: eframe::wgpu::BindingResource::TextureView(view.as_ref()),
                    },
                    eframe::wgpu::BindGroupEntry {
                        binding: 1,
                        resource: eframe::wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            }),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_tests(
        texture_id: egui::TextureId,
        size: [usize; 2],
        cleanup: impl Fn(egui::TextureId) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            texture_id,
            size,
            cleanup: Arc::new(cleanup),
            _texture: None,
            view: None,
        })
    }
}

impl Drop for NativeTextureHandle {
    fn drop(&mut self) {
        (self.cleanup)(self.texture_id);
    }
}

pub(super) fn build_preview_texture(
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
    preview: &crate::jobs::image_formats::PreviewImage,
) -> Result<ImportedPreviewTexture, String> {
    let Some(render_state) = wgpu_render_state else {
        return Ok(ImportedPreviewTexture::Cpu);
    };

    if preview.rgba.len() != preview.width as usize * preview.height as usize * 4 {
        return Err("preview pixel buffer did not match preview dimensions".to_owned());
    }

    let size = eframe::wgpu::Extent3d {
        width: preview.width,
        height: preview.height,
        depth_or_array_layers: 1,
    };
    let texture = render_state
        .device
        .create_texture(&eframe::wgpu::TextureDescriptor {
            label: Some("snappyref_import_preview"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: eframe::wgpu::TextureDimension::D2,
            format: eframe::wgpu::TextureFormat::Rgba8Unorm,
            usage: eframe::wgpu::TextureUsages::TEXTURE_BINDING
                | eframe::wgpu::TextureUsages::COPY_DST,
            view_formats: &[eframe::wgpu::TextureFormat::Rgba8Unorm],
        });
    render_state.queue.write_texture(
        texture.as_image_copy(),
        &preview.rgba,
        eframe::wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * preview.width),
            rows_per_image: Some(preview.height),
        },
        size,
    );

    let view = texture.create_view(&eframe::wgpu::TextureViewDescriptor::default());
    let texture_id = render_state.renderer.write().register_native_texture(
        &render_state.device,
        &view,
        eframe::wgpu::FilterMode::Linear,
    );

    Ok(ImportedPreviewTexture::Native(Arc::new(
        NativeTextureHandle::from_registered_texture(
            render_state.clone(),
            texture_id,
            preview.width,
            preview.height,
            texture,
            view,
        ),
    )))
}
