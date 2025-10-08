import { describe, it, expect } from "vitest";
import { isValidComponent } from "./test-utils";

describe("MafsModal Components", () => {
  it("should render CircleInputs component", async () => {
    const { default: CircleInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/CircleInputs"
    );
    expect(CircleInputs).toBeDefined();
    expect(isValidComponent(CircleInputs)).toBe(true);
  });

  it("should render EllipseInputs component", async () => {
    const { default: EllipseInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/EllipseInputs"
    );
    expect(EllipseInputs).toBeDefined();
    expect(isValidComponent(EllipseInputs)).toBe(true);
  });

  it("should render LatexInputs component", async () => {
    const { default: LatexInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/LatexInputs"
    );
    expect(LatexInputs).toBeDefined();
    expect(isValidComponent(LatexInputs)).toBe(true);
  });

  it("should render LineInputs component", async () => {
    const { default: LineInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/LineInputs"
    );
    expect(LineInputs).toBeDefined();
    expect(isValidComponent(LineInputs)).toBe(true);
  });

  it("should render PlotInputs component", async () => {
    const { default: PlotInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/PlotInputs"
    );
    expect(PlotInputs).toBeDefined();
    expect(isValidComponent(PlotInputs)).toBe(true);
  });

  it("should render PointInputs component", async () => {
    const { default: PointInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/PointInputs"
    );
    expect(PointInputs).toBeDefined();
    expect(isValidComponent(PointInputs)).toBe(true);
  });

  it("should render PolygonInputs component", async () => {
    const { default: PolygonInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/PolygonInputs"
    );
    expect(PolygonInputs).toBeDefined();
    expect(isValidComponent(PolygonInputs)).toBe(true);
  });

  it("should render TextInputs component", async () => {
    const { default: TextInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/TextInputs"
    );
    expect(TextInputs).toBeDefined();
    expect(isValidComponent(TextInputs)).toBe(true);
  });

  it("should render TextToLatex component", async () => {
    const { default: TextToLatex } = await import(
      "../../renderer/pages/components/partials/mafsmodal/TextToLatex"
    );
    expect(TextToLatex).toBeDefined();
    expect(isValidComponent(TextToLatex)).toBe(true);
  });

  it("should render VectorInputs component", async () => {
    const { default: VectorInputs } = await import(
      "../../renderer/pages/components/partials/mafsmodal/VectorInputs"
    );
    expect(VectorInputs).toBeDefined();
    expect(isValidComponent(VectorInputs)).toBe(true);
  });
});
