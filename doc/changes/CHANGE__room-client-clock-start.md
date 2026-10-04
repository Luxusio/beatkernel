# Common room client clock and start composition

RoomPlayClient composes actual room admission, symmetric clock probes and the
existing Join start agreement in the BMS app. A single global write identity
space and in-flight slot dispatch exact complete-write receipts to their
original child owner. Only validated Prepared membership permits clock/start
traffic. Fully completed probes install the measured offset before local
ClockReady/proposals; early genuine peer ClockReady uses the existing bounded
readiness state. A matching Commit after complete Accept produces one translated
schedule with actual local preroll. Leave and Stop fence continued control work.

Deferred fixtures cover common admission-to-start flows, exact receipt barriers,
invalid chronology/control refusal, early readiness and cancellation. Compile
checks will be recorded after both writers stop. No runtime acceptance is claimed.
Actual timed transport drivers, server control handling, WASM/Worker/Page output
activation, participant progress and final acknowledgements remain required.
The full BMS player Goal and independent review/security/QA remain open.
