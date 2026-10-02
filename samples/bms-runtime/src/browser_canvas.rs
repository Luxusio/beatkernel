//! Worker-owned WebGPU canvas using the common scene and prepared chart assets.

use std::sync::Arc;

use beatkernel::time::Timestamp;
use web_sys::OffscreenCanvas;

use crate::{
    bga_render::BgaTextureCache,
    graphics::{self, BackendChoice, Presentation, Renderer},
    image_assets::ImageAssets,
    player_chart::PlayerChart,
    poor_background::PoorBackgroundPolicy,
    scene::Scene,
    ui::organisms,
};

pub(crate) struct BrowserCanvas {
    canvas: OffscreenCanvas,
    instance: wgpu::Instance,
    renderer: Renderer,
    scene: Scene,
    backgrounds: BgaTextureCache,
    extent: [u32; 2],
}

impl BrowserCanvas {
    pub(crate) async fn create(canvas: OffscreenCanvas) -> Result<Self, String> {
        let extent = [canvas.width(), canvas.height()];
        let instance = graphics::instance(BackendChoice::Auto)?;
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas.clone()))
            .map_err(|error| format!("create browser canvas surface: {error}"))?;
        let mut renderer = Renderer::new(surface, &instance, Presentation::Fifo).await?;
        // Renderer checks the device limit before wgpu configures the canvas's
        // backing extent. A zero extent leaves the renderer suspended.
        renderer.resize(extent[0], extent[1])?;
        Ok(Self {
            canvas,
            instance,
            renderer,
            scene: Scene::new(960, 720),
            backgrounds: BgaTextureCache::default(),
            extent,
        })
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let extent = [width, height];
        if self.extent != extent {
            self.renderer.resize(width, height)?;
            self.extent = extent;
        }
        Ok(())
    }

    /// Displays an explicit original-song position, without advancing a clock,
    /// judging input, or inventing progress for the prepared chart.
    pub(crate) fn present_chart(
        &mut self,
        chart: &PlayerChart,
        images: &Arc<ImageAssets>,
        song: Timestamp,
        lookahead: i64,
    ) -> Result<(), String> {
        if lookahead <= 0 {
            return Err("playfield lookahead must be positive".into());
        }
        if self.renderer.needs_surface_recreation() {
            let surface = self
                .instance
                .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.canvas.clone()))
                .map_err(|error| format!("recreate browser canvas surface: {error}"))?;
            self.renderer.replace_surface(surface)?;
        }
        let presentation = PoorBackgroundPolicy::default().select(chart, song, None)?;
        // The common cache releases a previous asset bank and keeps current
        // image aliases, opacity and unavailable-image behavior intact.
        let frames = self.backgrounds.sync_presentations(
            Some(images),
            &[presentation],
            &mut self.renderer,
        )?;
        // Clearing geometry retains Scene's visible-note and GPU instance cache.
        self.scene.clear();
        organisms::playfield_with_background(
            &mut self.scene,
            chart,
            song,
            lookahead,
            &[],
            0,
            None,
            frames[0],
        )?;
        self.renderer.render(&self.scene)
    }

    /// The host owns bounded retry scheduling after transient surface failures.
    pub(crate) fn needs_redraw(&self) -> bool {
        self.renderer.needs_redraw()
    }
}
