pub fn render(
    scene: &animathio_core::Scene,
    ctx: &mut animathio_render::RenderCtx,
    size: (u32, u32),
) -> Result<animathio_render::Frame, animathio_render::Error> {
    ctx.set_effect_processor(animathio_plugin_host::test_support::effect_processor());
    animathio_render::render(
        &animathio_plugin_host::test_support::resolve(scene, size),
        ctx,
        size,
    )
}
