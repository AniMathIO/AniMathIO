import { describe, it, expect } from "vitest";
import { isValidComponent } from "./test-utils";

describe("Main Components", () => {
  it("should render Resources component", async () => {
    const { default: Resources } = await import(
      "../../renderer/pages/components/Resources"
    );
    expect(Resources).toBeDefined();
    expect(isValidComponent(Resources)).toBe(true);
  });

  it("should render Timeline component", async () => {
    const { default: Timeline } = await import(
      "../../renderer/pages/components/Timeline"
    );
    expect(Timeline).toBeDefined();
    expect(isValidComponent(Timeline)).toBe(true);
  });

  it("should render Titlebar component (in-app menu-bar fallback)", async () => {
    const { default: Titlebar } = await import(
      "../../renderer/pages/components/Titlebar"
    );
    expect(Titlebar).toBeDefined();
    expect(isValidComponent(Titlebar)).toBe(true);
  });

  it("should render ManimImportPanel component", async () => {
    const { default: ManimImportPanel } = await import(
      "../../renderer/pages/components/panels/ManimImportPanel"
    );
    expect(ManimImportPanel).toBeDefined();
    expect(isValidComponent(ManimImportPanel)).toBe(true);
  });
});
