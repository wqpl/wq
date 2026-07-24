use wqpl::wqdb::{ChunkId, CodeLoc, DebugInfo};

use super::format_breakpoint_loc;

#[test]
fn stale_breakpoint_locations_render_without_panicking() {
    let debug_info = DebugInfo::default();
    let location = CodeLoc {
        chunk: ChunkId(u32::MAX),
        pc: 7,
    };

    assert_eq!(
        format_breakpoint_loc(&debug_info, location),
        "pc 7 (location unavailable)"
    );
}
