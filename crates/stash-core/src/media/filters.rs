//! ffmpeg filter snippets shared by thumbnails and live transcoding.

/// HDR → SDR tone mapping (zscale + tonemap hable), ending in 8-bit `yuv420p`.
///
/// `hdr` is the summary's HDR kind. HDR10 and HLG name their input transfer explicitly so
/// files without colour metadata still convert; Dolby Vision uses what the decoder reports.
pub fn tonemap_chain(hdr: &str) -> String {
    let input = match hdr {
        "hlg" => ":tin=arib-std-b67:pin=bt2020:min=bt2020nc",
        "hdr10" => ":tin=smpte2084:pin=bt2020:min=bt2020nc",
        _ => "",
    };
    format!(
        "zscale=t=linear:npl=100{input},format=gbrpf32le,zscale=p=bt709,\
         tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
    )
}

/// Square pixels, then fit inside a `long_edge` box without upscaling.
pub fn fit_box(long_edge: u32) -> String {
    format!(
        "scale=iw*sar:ih,setsar=1,\
         scale=w='min({long_edge},iw)':h='min({long_edge},ih)':force_original_aspect_ratio=decrease"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tonemap_variants() {
        assert!(tonemap_chain("hdr10").contains("tin=smpte2084"));
        assert!(tonemap_chain("hlg").contains("tin=arib-std-b67"));
        let dv = tonemap_chain("dolby_vision");
        assert!(!dv.contains("tin="));
        assert!(dv.contains("tonemap=tonemap=hable"));
        assert!(dv.ends_with("format=yuv420p"));
    }

    #[test]
    fn fit_box_never_upscales() {
        let f = fit_box(400);
        assert!(f.contains("min(400,iw)"));
        assert!(f.contains("force_original_aspect_ratio=decrease"));
    }
}
