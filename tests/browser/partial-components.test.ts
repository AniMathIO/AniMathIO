import { describe, it, expect } from "vitest";
import { isValidComponent } from "./test-utils";

describe("Partial Components", () => {
  it("should render MafsModal component", async () => {
    const { default: MafsModal } = await import(
      "../../renderer/pages/components/partials/MafsModal"
    );
    expect(MafsModal).toBeDefined();
    expect(isValidComponent(MafsModal)).toBe(true);
  });

  it("should render UploadButton component", async () => {
    const { default: UploadButton } = await import(
      "../../renderer/pages/components/partials/shared/UploadButton"
    );
    expect(UploadButton).toBeDefined();
    expect(isValidComponent(UploadButton)).toBe(true);
  });

  it("should render Dragable component", async () => {
    const { default: Dragable } = await import(
      "../../renderer/pages/components/partials/timeline/Dragable"
    );
    expect(Dragable).toBeDefined();
    expect(isValidComponent(Dragable)).toBe(true);
  });

  it("should render ScaleRangeInput component", async () => {
    const { default: ScaleRangeInput } = await import(
      "../../renderer/pages/components/partials/timeline/ScaleRangeInput"
    );
    expect(ScaleRangeInput).toBeDefined();
    expect(isValidComponent(ScaleRangeInput)).toBe(true);
  });

  it("should render SeekPlayer component", async () => {
    const { default: SeekPlayer } = await import(
      "../../renderer/pages/components/partials/timeline/SeekPlayer"
    );
    expect(SeekPlayer).toBeDefined();
    expect(isValidComponent(SeekPlayer)).toBe(true);
  });

  it("should render Timeframe component", async () => {
    const { default: Timeframe } = await import(
      "../../renderer/pages/components/partials/timeline/Timeframe"
    );
    expect(Timeframe).toBeDefined();
    expect(isValidComponent(Timeframe)).toBe(true);
  });
});
