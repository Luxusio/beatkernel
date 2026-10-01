# Menu interaction built from portable components

Portable logical hit rectangles and press/release/cancel state now support a
button molecule and real BMS catalog/start/cancel/return/exit controls. Mouse
coordinates use the same logical viewport stretch as GPU rendering, with
non-finite positions, invalid extents and outside edges rejected. Press and
release must hit the same control; focus loss, resizing, suspension and close
cancel gestures. These are menu commands, preserving the existing native input
clock and judging path. Keyboard navigation remains available, and session
cleanup still precedes another run. Pure fixtures are authored and compiled;
actual GUI/pointer/focus/audio execution and independent acceptance remain
deferred under the user's existing instruction. The full player Goal remains
unfinished.
