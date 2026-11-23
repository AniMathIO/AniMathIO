"""A Manim Community scene for the AniMathIO importer."""

from manim import BLUE, LEFT, RIGHT, UP, Circle, Create, MathTex, Scene, Text, Write


class Gallery(Scene):
    def construct(self):
        title = Text("Manim scene import", font_size=61)
        title.shift(2.3 * UP)
        circle = Circle(radius=1.5, color=BLUE, fill_color="#244769", fill_opacity=1)
        equation = MathTex(r"e^{i\pi} + 1 = 0", font_size=77)
        equation.shift(2.8 * RIGHT)
        self.play(Write(title))
        self.play(Create(circle))
        self.play(circle.animate.shift(2.8 * LEFT))
        self.play(Write(equation))
        self.wait(1)
