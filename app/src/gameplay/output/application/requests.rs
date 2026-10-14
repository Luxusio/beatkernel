//! Cold correlated UI admission, separate from output retirement and publication.
use crate::gameplay::output::ports::OutputUiPort;
use crate::{
    gameplay::output::application::owner::{
        CoordinateSourceOwnerPublication, CoordinateTargetOwnerPublication, GameplayOutputOwner,
    },
    gameplay::output::domain::control::{OutputCapability, OutputReply, OutputRequest},
    gameplay::output::ports::{
        ConditionalSourceRetirementNativeOutputBackend,
        ConditionalTargetRetirementNativeOutputBackend, OriginalNativeOutputBackend,
        OriginalTargetNativeOutputBackend, OutputReplacementBackend, SourcePlaybackIdentity,
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayOutputContext},
};
use beatkernel::{
    audio::{OutputFrameBasis, SoftwareOutputState, TargetFrameBasis},
    time::ClockPoint,
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, NativeOutputState};
use std::io;
pub struct GameplayOutputUi<U: OutputUiPort> {
    ui: U,
    flight: Option<u64>,
    reply: Option<OutputReply>,
}
impl<U: OutputUiPort> GameplayOutputUi<U> {
    pub fn new(ui: U) -> Self {
        Self {
            ui,
            flight: None,
            reply: None,
        }
    }
    pub fn pending(&self) -> bool {
        self.flight.is_some() || self.reply.is_some() || self.ui.pending()
    }
    pub fn advertise(&mut self, cap: Option<OutputCapability>) -> io::Result<()> {
        self.ui.advertise(cap)
    }
    fn flush(&mut self) -> io::Result<bool> {
        let Some(reply) = &self.reply else {
            return Ok(true);
        };
        match self.ui.reply(reply) {
            Ok(()) => {
                self.reply = None;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub fn service<B: OutputReplacementBackend<O>, O: SoftwareOutputState>(
        &mut self,
        owner: &mut GameplayOutputOwner<B, O>,
        context: GameplayOutputContext<'_, B::Presentation>,
        now: ClockPoint,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
        applied: &mut impl FnMut(&B::Output) -> Result<OutputCapability, String>,
    ) -> Result<bool, Box<dyn std::error::Error>>
    where
        B::Error: std::error::Error + 'static,
    {
        self.service_with(owner, None, map, applied, |owner| {
            owner.publish_paused(context, now)
        })
    }
    /// Join the same correlated UI request to original-evidence audio publication.
    pub fn service_audio<B: OriginalNativeOutputBackend<O>, O: SoftwareOutputState>(
        &mut self,
        owner: &mut GameplayOutputOwner<B, O>,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
        applied: &mut impl FnMut(&B::Output) -> Result<OutputCapability, String>,
    ) -> Result<bool, Box<dyn std::error::Error>>
    where
        B::Error: std::error::Error + 'static,
    {
        let refusal = context
            .presentation
            .has_retained_practice()
            .then_some("output replacement is unavailable during retained practice");
        self.service_with(owner, refusal, map, applied, |owner| {
            owner.publish_paused_audio(context, now)
        })
    }
    /// Keep target publication on the original converted owner and physical basis.
    pub fn service_target_audio<
        B: OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>,
        Ticket,
    >(
        &mut self,
        owner: &mut GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, Ticket>,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
        applied: &mut impl FnMut(&B::Output) -> Result<OutputCapability, String>,
    ) -> Result<bool, Box<dyn std::error::Error>>
    where
        B::Error: std::error::Error + 'static,
    {
        let refusal = context
            .presentation
            .has_retained_practice()
            .then_some("output replacement is unavailable during retained practice");
        self.service_with(owner, refusal, map, applied, |owner| {
            owner.publish_paused_target_audio(context, now)
        })
    }
    /// Stage the exact applied capability before committing a coordinate epoch.
    pub fn service_coordinate_target_audio<
        B: ConditionalTargetRetirementNativeOutputBackend<ConvertedNativeOutputState>,
    >(
        &mut self,
        owner: &mut GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, B::Ticket>,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        identity: SourcePlaybackIdentity,
        practice_pending: bool,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
        applied: &mut impl FnMut(&B::Output) -> Result<OutputCapability, String>,
    ) -> crate::native_gameplay::NativeGameplayResult<bool>
    where
        B::Error: std::error::Error + 'static,
    {
        let refusal = (practice_pending || context.end.is_some())
            .then_some("coordinate replacement requires no NativeEnd or pending practice");
        if !self.admit_request(owner, refusal, map)? {
            return Ok(false);
        }
        let publication = owner.publish_paused_coordinate_target_audio(
            context,
            now,
            identity,
            practice_pending,
            |output| {
                let cap = applied(output)?;
                cap.validate()?;
                Ok(cap)
            },
        );
        match publication {
            Ok(CoordinateTargetOwnerPublication::Pending) => Ok(false),
            Ok(CoordinateTargetOwnerPublication::LiveRefused(reason)) => {
                let id = self
                    .flight
                    .take()
                    .expect("active coordinate output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Err(Self::diagnostic(&reason)),
                });
                let _ = self.flush()?;
                Ok(false)
            }
            Ok(CoordinateTargetOwnerPublication::Published(cap)) => {
                let id = self
                    .flight
                    .take()
                    .expect("accepted coordinate output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Ok(cap),
                });
                let _ = self.flush()?;
                Ok(true)
            }
            Err(error) => {
                let id = self
                    .flight
                    .take()
                    .expect("active coordinate output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Err(Self::diagnostic(&error.to_string())),
                });
                let _ = self.flush();
                Err(error)
            }
        }
    }
    /// Stage the exact applied capability before committing a Source coordinate epoch.
    pub fn service_coordinate_source_audio<
        B: ConditionalSourceRetirementNativeOutputBackend<NativeOutputState>,
    >(
        &mut self,
        owner: &mut GameplayOutputOwner<B, NativeOutputState, OutputFrameBasis, B::Ticket>,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        identity: SourcePlaybackIdentity,
        practice_pending: bool,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
        applied: &mut impl FnMut(&B::Output) -> Result<OutputCapability, String>,
    ) -> crate::native_gameplay::NativeGameplayResult<bool>
    where
        B::Error: std::error::Error + 'static,
    {
        let refusal = (practice_pending || context.end.is_some())
            .then_some("coordinate replacement requires no NativeEnd or pending practice");
        if !self.admit_request(owner, refusal, map)? {
            return Ok(false);
        }
        let publication = owner.publish_paused_coordinate_source_audio(
            context,
            now,
            identity,
            practice_pending,
            |output| {
                let cap = applied(output)?;
                cap.validate()?;
                Ok(cap)
            },
        );
        match publication {
            Ok(CoordinateSourceOwnerPublication::Pending) => Ok(false),
            Ok(CoordinateSourceOwnerPublication::LiveRefused(reason)) => {
                let id = self
                    .flight
                    .take()
                    .expect("active coordinate output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Err(Self::diagnostic(&reason)),
                });
                let _ = self.flush()?;
                Ok(false)
            }
            Ok(CoordinateSourceOwnerPublication::Published(cap)) => {
                let id = self
                    .flight
                    .take()
                    .expect("accepted coordinate output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Ok(cap),
                });
                let _ = self.flush()?;
                Ok(true)
            }
            Err(error) => {
                let id = self
                    .flight
                    .take()
                    .expect("active coordinate output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Err(Self::diagnostic(&error.to_string())),
                });
                let _ = self.flush();
                Err(error)
            }
        }
    }
    fn diagnostic(error: &str) -> String {
        error
            .chars()
            .filter(|ch| !ch.is_control())
            .take(512)
            .collect()
    }
    fn admit_request<B: OutputReplacementBackend<O, Basis>, O, Basis, Ticket>(
        &mut self,
        owner: &mut GameplayOutputOwner<B, O, Basis, Ticket>,
        refusal: Option<&'static str>,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
    ) -> Result<bool, Box<dyn std::error::Error>>
    where
        B::Error: std::error::Error + 'static,
    {
        if !self.flush()? {
            return Ok(false);
        }
        if self.flight.is_none() {
            let request = match self.ui.take_request() {
                Ok(Some(request)) => request,
                Ok(None) => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) => return Err(error.into()),
            };
            if let Some(error) = refusal {
                self.reply = Some(OutputReply {
                    id: request.id,
                    result: Err(error.into()),
                });
                let _ = self.flush()?;
                return Ok(false);
            }
            let typed = owner
                .current()
                .ok_or_else(|| "current output unavailable".to_string())
                .and_then(|output| map(&request, output));
            match typed {
                Ok(typed) => {
                    if owner.queue(typed, 2_000_000_000).is_err() {
                        self.reply = Some(OutputReply {
                            id: request.id,
                            result: Err("output replacement is busy".into()),
                        });
                    } else {
                        self.flight = Some(request.id);
                    }
                }
                Err(error) => {
                    self.reply = Some(OutputReply {
                        id: request.id,
                        result: Err(error),
                    })
                }
            }
            if self.flight.is_none() {
                let _ = self.flush()?;
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn service_with<B: OutputReplacementBackend<O, Basis>, O, Basis, Ticket>(
        &mut self,
        owner: &mut GameplayOutputOwner<B, O, Basis, Ticket>,
        refusal: Option<&'static str>,
        map: &mut impl FnMut(&OutputRequest, &B::Output) -> Result<B::Request, String>,
        applied: &mut impl FnMut(&B::Output) -> Result<OutputCapability, String>,
        publish: impl FnOnce(
            &mut GameplayOutputOwner<B, O, Basis, Ticket>,
        ) -> Result<bool, Box<dyn std::error::Error>>,
    ) -> Result<bool, Box<dyn std::error::Error>>
    where
        B::Error: std::error::Error + 'static,
    {
        if !self.admit_request(owner, refusal, map)? {
            return Ok(false);
        }
        match publish(owner) {
            Ok(false) => Ok(false),
            Ok(true) => {
                let id = self.flight.take().expect("accepted output request");
                let result = owner
                    .current()
                    .ok_or_else(|| "published output unavailable".into())
                    .and_then(applied);
                self.reply = Some(OutputReply { id, result });
                let _ = self.flush()?;
                Ok(true)
            }
            Err(error) => {
                let id = self.flight.take().expect("active output request");
                self.reply = Some(OutputReply {
                    id,
                    result: Err(Self::diagnostic(&error.to_string())),
                });
                let _ = self.flush();
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "audio_requests_fixtures.rs"]
mod audio_fixtures;

#[cfg(test)]
#[path = "retained_target_requests_fixtures.rs"]
mod retained_target_requests_fixtures;

#[cfg(test)]
#[path = "retained_source_requests_fixtures.rs"]
mod retained_source_requests_fixtures;
