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
    ui::{atoms, molecules, organisms, interaction::Bounds},
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
        self.present(
            chart,
            images,
            song,
            lookahead,
            &[],
            0,
            None,
            None,
            None,
            None,
        )
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
            Some(game.game.gauge()),
            Some(&game.saved_hud),
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
            Some(replay.replay.gauge()),
            None,
        )
    }

    pub(crate) fn present_local_game(
        &mut self,
        game: &crate::browser_local_game::BrowserLocalGame,
        lookahead: i64,
        page: usize,
    ) -> Result<(), String> {
        let count = game.members.len();
        let page_size = organisms::LOCAL_PLAYERS_PER_PAGE;
        if lookahead <= 0
            || !(1..=crate::local_players::MAX_LOCAL_PLAYERS).contains(&count)
            || page >= count.div_ceil(page_size)
        {
            return Err("invalid browser local roster, page or lookahead".into());
        }
        let first = page * page_size;
        let visible = first..(first + page_size).min(count);
        // Borrow actual state, including full-prefix note progress and score maps.
        // This stack-only roster never copies per-note arrays or recent results.
        let member_view = |index: usize| {
            let member = &game.members[index];
            organisms::LocalPlayerView {
                bms_score: None,
                player: member.player,
                chart: Some(&game.chart),
                song_time: game.game.member_song_time(member.player),
                score: game
                    .game
                    .score(member.player)
                    .expect("prepared local member"),
                gauge: game.game.gauge(member.player),
                last_judge: member.recent.last(),
                recent_results: &member.recent,
                pressed_lanes: member.pressed,
                note_progress: Some(&member.progress),
                competition: member.saved_hud.snapshot(),
            }
        };
        let mut views = [member_view(0); crate::local_players::MAX_LOCAL_PLAYERS];
        let mut comparison_space = [0; crate::local_players::MAX_LOCAL_PLAYERS];
        for (index, destination) in views[..count].iter_mut().enumerate() {
            *destination = member_view(index);
            comparison_space[index] = game.members[index].comparison_height();
        }
        let mut presentations = [crate::poor_background::BgaPresentation::default(); 4];
        for (destination, index) in presentations.iter_mut().zip(visible.clone()) {
            let view = views[index];
            let song = view.song_time.ok_or("local member has no song frontier")?;
            *destination =
                PoorBackgroundPolicy::default().select(&game.chart, song, view.note_progress)?;
        }
        if self.renderer.needs_surface_recreation() {
            let surface = self
                .instance
                .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.canvas.clone()))
                .map_err(|error| format!("recreate browser canvas surface: {error}"))?;
            self.renderer.replace_surface(surface)?;
        }
        let frames = self.backgrounds.sync_presentations(
            Some(&game.images),
            &presentations[..visible.len()],
            &mut self.renderer,
        )?;
        self.scene.clear();
        organisms::local_player_views_with_reserved_comparison_space(
            &mut self.scene,
            &views[..count],
            lookahead,
            page,
            true,
            &frames,
            &comparison_space[..count],
        )?;
        for (slot, index) in visible.enumerate() {
            let member = &game.members[index];
            let saved_space = member.saved_comparison_height();
            let saved_failed = member.saved_hud.failed() && saved_space > 0;
            let peer_failed = member.peer_admitted && member.saved_hud.peer_failed();
            if saved_failed || peer_failed {
                let [x, y, _, _] = crate::playfield_layout::local_panel_bounds(
                    (count - first).min(page_size),
                    slot,
                )?;
                if saved_failed {
                    atoms::text(
                        &mut self.scene,
                        (x + 10) as usize,
                        (y + 72) as usize,
                        "SAVED COMPARISONS UNAVAILABLE",
                        1,
                        0xff8e8e,
                    );
                }
                if peer_failed {
                    atoms::text(
                        &mut self.scene,
                        (x + 10) as usize,
                        (y + 72 + saved_space) as usize,
                        "PEER DISPLAY UNAVAILABLE",
                        1,
                        0xff8e8e,
                    );
                }
            }
        }
        if let Some(hud) = &game.room_hud {
            organisms::room_opponent_footer(&mut self.scene, hud)?;
        } else if game.room_hud_disabled {
            atoms::text(
                &mut self.scene,
                12,
                646,
                "ROOM SCORES UNAVAILABLE",
                1,
                0xff8e8e,
            );
        }
        self.renderer.render(&self.scene)
    }

    pub(crate) fn present_historical_record(
        &mut self,
        record: &crate::browser_historical_record::BrowserHistoricalRecord,
    ) -> Result<(), String> {
        if self.renderer.needs_surface_recreation() {
            let surface = self
                .instance
                .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.canvas.clone()))
                .map_err(|error| format!("recreate browser canvas surface: {error}"))?;
            self.renderer.replace_surface(surface)?;
        }
        self.backgrounds
            .sync_presentations(None, &[], &mut self.renderer)?;
        let presentation = record
            .presentation
            .as_ref()
            .ok_or("historical record display unavailable")?;
        self.scene.clear();
        presentation.compose(&mut self.scene)?;
        self.renderer.render(&self.scene)
    }

    pub(crate) fn present_completed_results(
        &mut self,
        results: &crate::browser_completed_results::BrowserCompletedResults,
        room: Option<&crate::room_presentation::RoomPresentation>,
    ) -> Result<(), String> {
        if self.renderer.needs_surface_recreation() {
            let surface = self
                .instance
                .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.canvas.clone()))
                .map_err(|error| format!("recreate browser canvas surface: {error}"))?;
            self.renderer.replace_surface(surface)?;
        }
        self.backgrounds
            .sync_presentations(None, &[], &mut self.renderer)?;
        let view = results
            .presentation
            .view()
            .ok_or("completed Results display unavailable")?;
        self.scene.clear();
        crate::ui::atoms::text(
            &mut self.scene,
            24,
            65,
            if results.has_comparisons() {
                "COMPLETED RESULTS - C COMPARISONS"
            } else {
                "COMPLETED RESULTS"
            },
            2,
            0x9bb1cf,
        );
        view.compose_mode(
            &mut self.scene,
            results.page() as usize,
            results.comparisons(),
        )?;
        if let Some(room) = room {
            organisms::room_presentation_footer(&mut self.scene, room)?;
        }
        self.renderer.render(&self.scene)
    }

    pub(crate) fn present_room_results(
        &mut self,
        page: &crate::room_presentation::RoomPresentation,
    ) -> Result<(), String> {
        if self.renderer.needs_surface_recreation() {
            let surface = self
                .instance
                .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(self.canvas.clone()))
                .map_err(|error| format!("recreate browser canvas surface: {error}"))?;
            self.renderer.replace_surface(surface)?;
        }
        self.backgrounds
            .sync_presentations(None, &[], &mut self.renderer)?;
        self.scene.clear();
        organisms::room_presentation_footer(&mut self.scene, page)?;
        self.renderer.render(&self.scene)
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
        gauge: Option<&crate::gauge::BmsGauge>,
        saved_hud: Option<&crate::saved_opponent_hud::SavedOpponentHud>,
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
            if let Some(snapshot) = saved_hud.and_then(|hud| hud.snapshot()) {
                organisms::competition_scoreboard(&mut self.scene, score, snapshot)?;
            } else {
                organisms::scoreboard(&mut self.scene, score, recent);
            }
            if saved_hud.is_some_and(|hud| hud.failed()) {
                atoms::text(&mut self.scene, 750, 650, "SAVED COMPARISONS", 1, 0xff8e8e);
                atoms::text(&mut self.scene, 750, 660, "UNAVAILABLE", 1, 0xff8e8e);
            }
        }
        if let Some(gauge) = gauge {
            molecules::gauge_hud(
                &mut self.scene,
                gauge,
                Bounds {
                    x: 750,
                    y: 110,
                    width: 186,
                    height: 18,
                },
            )?;
        }
        self.renderer.render(&self.scene)
    }

    /// The host owns bounded retry scheduling after transient surface failures.
    pub(crate) fn needs_redraw(&self) -> bool {
        self.renderer.needs_redraw()
    }
}
