//! Worker-owned WebGPU canvas using the common scene and prepared chart assets.

use std::sync::Arc;

use beatkernel::time::Timestamp;
use web_sys::OffscreenCanvas;

use crate::{
    bga_render::BgaTextureCache,
    graphics::{self, BackendChoice, Presentation, Renderer},
    image_assets::ImageAssets,
    player_chart::PlayerChart,
    playfield_layout::LOGICAL_EXTENT,
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
            scene: Scene::new(LOGICAL_EXTENT[0], LOGICAL_EXTENT[1]),
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
        self.present(chart, images, song, lookahead, &[], 0, None, None)
    }

    pub(crate) fn present_game(
        &mut self,
        game: &crate::browser_game::BrowserGame,
        lookahead: i64,
    ) -> Result<(), String> {
        self.present(
            &game.chart,
            &game.images,
            game.game.song_time(),
            lookahead,
            &game.recent,
            game.pressed,
            Some(&game.progress),
            Some(game.game.score()),
        )
    }

    pub(crate) fn present_replay(
        &mut self,
        replay: &crate::browser_replay::BrowserReplay,
        lookahead: i64,
    ) -> Result<(), String> {
        self.present(
            &replay.chart,
            &replay.images,
            replay.replay.song_time(),
            lookahead,
            &replay.recent,
            replay.replay.pressed_lanes(),
            Some(&replay.progress),
            Some(replay.replay.score()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn present(
        &mut self,
        chart: &PlayerChart,
        images: &Arc<ImageAssets>,
        song: Timestamp,
        lookahead: i64,
        recent: &[beatkernel::judge::JudgeEvent],
        pressed: u32,
        progress: Option<&crate::note_progress::NoteProgress>,
        score: Option<&crate::competition::ScoreSummary>,
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
        let presentation = PoorBackgroundPolicy::default().select(chart, song, progress)?;
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
            recent,
            pressed,
            progress,
            frames[0],
        )?;
        if let Some(score) = score {
            organisms::scoreboard(&mut self.scene, score, recent);
        }
        self.renderer.render(&self.scene)
    }

    /// The host owns bounded retry scheduling after transient surface failures.
    pub(crate) fn needs_redraw(&self) -> bool {
        self.renderer.needs_redraw()
    }
}
