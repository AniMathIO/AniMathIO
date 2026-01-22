import { describe, it, expect } from "vitest";
import { isValidComponent } from "./test-utils";

describe("Entity Components", () => {
  it("should render AnimationResource component", async () => {
    const { default: AnimationResource } = await import(
      "../../renderer/pages/components/entity/AnimationResource"
    );
    expect(AnimationResource).toBeDefined();
    expect(isValidComponent(AnimationResource)).toBe(true);
  });

  it("should render AudioRecorder component", async () => {
    const { default: AudioRecorder } = await import(
      "../../renderer/pages/components/entity/AudioRecorder"
    );
    expect(AudioRecorder).toBeDefined();
    expect(isValidComponent(AudioRecorder)).toBe(true);
  });

  it("should render AudioResource component", async () => {
    const { default: AudioResource } = await import(
      "../../renderer/pages/components/entity/AudioResource"
    );
    expect(AudioResource).toBeDefined();
    expect(isValidComponent(AudioResource)).toBe(true);
  });

  it("should render AudioTrack component", async () => {
    const { default: AudioTrack } = await import(
      "../../renderer/pages/components/entity/AudioTrack"
    );
    expect(AudioTrack).toBeDefined();
    expect(isValidComponent(AudioTrack)).toBe(true);
  });

  it("should render EffectResource component", async () => {
    const { default: EffectResource } = await import(
      "../../renderer/pages/components/entity/EffectResource"
    );
    expect(EffectResource).toBeDefined();
    expect(isValidComponent(EffectResource)).toBe(true);
  });

  it("should render Element component", async () => {
    const { default: Element } = await import(
      "../../renderer/pages/components/entity/Element"
    );
    expect(Element).toBeDefined();
    expect(isValidComponent(Element)).toBe(true);
  });

  it("should render ImageResource component", async () => {
    const { default: ImageResource } = await import(
      "../../renderer/pages/components/entity/ImageResource"
    );
    expect(ImageResource).toBeDefined();
    expect(isValidComponent(ImageResource)).toBe(true);
  });

  it("should render MafsResource component", async () => {
    const { default: MafsResource } = await import(
      "../../renderer/pages/components/entity/MafsResource"
    );
    expect(MafsResource).toBeDefined();
    expect(isValidComponent(MafsResource)).toBe(true);
  });

  it("should render TextResource component", async () => {
    const { default: TextResource } = await import(
      "../../renderer/pages/components/entity/TextResource"
    );
    expect(TextResource).toBeDefined();
    expect(isValidComponent(TextResource)).toBe(true);
  });

  it("should render VideoResource component", async () => {
    const { default: VideoResource } = await import(
      "../../renderer/pages/components/entity/VideoResource"
    );
    expect(VideoResource).toBeDefined();
    expect(isValidComponent(VideoResource)).toBe(true);
  });
});
