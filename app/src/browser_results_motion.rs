//! Renderer-owned Results presentation motion; completion data remains immutable.
use crate::{
    scene::{Scene, UiComponentKey, UiTransform, MAX_UI_COMPONENTS},
    screen_lifecycle::ScreenInstanceId,
    ui::{
        layout::NodeId,
        motion::{ComponentMotion, MotionScheduler},
        results::FrozenResultsView,
    },
};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct BrowserResultsMotion {
    identity: (u64, u64),
    screen: ScreenInstanceId,
    scheduler: MotionScheduler,
    nodes: Vec<(NodeId, UiTransform)>,
    time: Duration,
    suspended: bool,
    disposed: bool,
}
impl BrowserResultsMotion {
    pub(crate) fn new(identity: (u64, u64), screen: ScreenInstanceId) -> Result<Self, String> {
        if identity.0 == 0 || identity.1 == 0 {
            return Err("invalid Results registration identity".into());
        }
        Ok(Self {
            identity,
            screen,
            scheduler: MotionScheduler::new(screen, MAX_UI_COMPONENTS)?,
            nodes: Vec::new(),
            time: Duration::ZERO,
            suspended: false,
            disposed: false,
        })
    }
    pub(crate) fn identity(&self) -> (u64, u64) {
        self.identity
    }
    pub(crate) fn screen(&self) -> ScreenInstanceId {
        self.screen
    }
    pub(crate) fn nodes(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|(node, _)| *node).collect()
    }
    pub(crate) fn time(&self) -> Duration {
        self.time
    }
    pub(crate) fn validate_time(&self, now: Duration) -> Result<(), String> {
        if self.disposed || now < self.time {
            return Err("disposed Results owner or regressed time".into());
        }
        self.scheduler.validate_time(now)
    }
    pub(crate) fn request(
        &mut self,
        identity: (u64, u64),
        node: NodeId,
        motion: ComponentMotion,
        now: Duration,
        view: &FrozenResultsView,
        page: usize,
        comparisons: bool,
        scene: &mut Scene,
    ) -> Result<(), String> {
        self.validate_request(identity, node, now, view, page, comparisons)?;
        if scene.logical_extent().contains(&0) {
            return Err("Results motion node is not displayed".into());
        }
        let mut owner = self.clone();
        if !owner.nodes.iter().any(|(old, _)| *old == node) {
            if owner.nodes.len() == MAX_UI_COMPONENTS {
                return Err("Results motion capacity exhausted".into());
            }
            owner
                .nodes
                .try_reserve(1)
                .map_err(|_| "Results motion allocation failed")?;
            owner.nodes.push((node, UiTransform::default()));
        }
        let mut candidate = scene.component_render_candidate()?;
        view.compose_components_mode(
            &mut candidate,
            owner.screen,
            page,
            comparisons,
            &owner.nodes(),
        )?;
        owner.restore(&mut candidate)?;
        let key = UiComponentKey {
            screen: owner.screen,
            node,
        };
        let id = candidate
            .component_live(key)
            .ok_or("Results component unavailable")?;
        owner.scheduler.schedule(key, id, motion, now)?;
        owner.tick(now, scene.logical_extent(), &mut candidate)?;
        scene.publish_render_candidate(candidate)?;
        *self = owner;
        Ok(())
    }
    pub(crate) fn validate_request(
        &self,
        identity: (u64, u64),
        node: NodeId,
        now: Duration,
        view: &FrozenResultsView,
        page: usize,
        comparisons: bool,
    ) -> Result<(), String> {
        self.validate_time(now)?;
        if identity != self.identity || self.suspended {
            return Err("foreign or suspended Results motion request".into());
        }
        if !view.displayed_nodes(page, comparisons)?.contains(&node) {
            return Err("Results motion node is not displayed".into());
        }
        if self.nodes.len() == MAX_UI_COMPONENTS && !self.nodes.iter().any(|(old, _)| *old == node)
        {
            return Err("Results motion capacity exhausted".into());
        }
        Ok(())
    }
    pub(crate) fn prune(
        &mut self,
        view: &FrozenResultsView,
        page: usize,
        comparisons: bool,
    ) -> Result<bool, String> {
        if self.disposed {
            return Err("disposed Results owner".into());
        }
        let displayed = view.displayed_nodes(page, comparisons)?;
        let old = self.nodes.len();
        self.nodes.retain(|(node, _)| {
            if displayed.contains(node) {
                true
            } else {
                self.scheduler.cancel(*node);
                false
            }
        });
        Ok(old != self.nodes.len())
    }
    pub(crate) fn restore(&mut self, scene: &mut Scene) -> Result<(), String> {
        self.scheduler.restore_poses(scene, &self.nodes)?;
        Ok(())
    }
    pub(crate) fn tick(
        &mut self,
        now: Duration,
        extent: [u32; 2],
        scene: &mut Scene,
    ) -> Result<bool, String> {
        self.validate_time(now)?;
        if extent.contains(&0) {
            self.suspend(now)?;
            return Ok(false);
        }
        if self.suspended {
            self.time = now;
            return Ok(false);
        }
        if self.nodes.iter().any(|(node, _)| {
            scene
                .component_live(UiComponentKey {
                    screen: self.screen,
                    node: *node,
                })
                .and_then(|id| scene.component_transform(id))
                .is_none()
        }) {
            return Err("Results pose binding unavailable".into());
        }
        let changed = self.scheduler.tick(self.screen, now, scene)?;
        for (node, pose) in &mut self.nodes {
            *pose = scene
                .component_live(UiComponentKey {
                    screen: self.screen,
                    node: *node,
                })
                .and_then(|id| scene.component_transform(id))
                .ok_or("Results pose binding unavailable")?;
        }
        self.time = now;
        Ok(changed)
    }
    pub(crate) fn active(&self) -> bool {
        !self.disposed && !self.suspended && self.scheduler.active_count() > 0
    }
    pub(crate) fn suspend(&mut self, now: Duration) -> Result<(), String> {
        self.validate_time(now)?;
        self.scheduler.suspend(now)?;
        self.suspended = true;
        self.time = now;
        Ok(())
    }
    pub(crate) fn resume(&mut self, now: Duration) -> Result<(), String> {
        self.validate_time(now)?;
        self.scheduler.resume(now)?;
        self.suspended = false;
        self.time = now;
        Ok(())
    }
    pub(crate) fn dispose(&mut self) {
        self.scheduler.dispose();
        self.nodes.clear();
        self.disposed = true;
        self.suspended = false;
    }
}
