"use client";
import React from "react";
import { StateContext } from "../states";
import { observer } from "mobx-react";
import { ExportVideoPanel } from "./partials/panels/ExportVideoPanel";
import { AnimationsPanel } from "./partials/panels/AnimationsPanel";
import { AudioResourcesPanel } from "./partials/panels/AudioResourcesPanel";
import { FillPanel } from "./partials/panels/FillPanel";
import { ImageResourcesPanel } from "./partials/panels/ImageResourcesPanel";
import { TextResourcesPanel } from "./partials/panels/TextResourcesPanel";
import { VideoResourcesPanel } from "./partials/panels/VideoResourcesPanel";
import { EffectsPanel } from "./partials/panels/EffectsPanel";

export const Resources = observer(() => {
    const state = React.useContext(StateContext);
    const selectedMenuOption = state.selectedMenuOption;
    return (
        <div className="bg-slate-200 h-full">
            {selectedMenuOption === "Video" ? <VideoResourcesPanel /> : null}
            {selectedMenuOption === "Audio" ? <AudioResourcesPanel /> : null}
            {selectedMenuOption === "Image" ? <ImageResourcesPanel /> : null}
            {selectedMenuOption === "Text" ? <TextResourcesPanel /> : null}
            {selectedMenuOption === "Animation" ? <AnimationsPanel /> : null}
            {selectedMenuOption === "Effect" ? <EffectsPanel /> : null}
            {selectedMenuOption === "Export" ? <ExportVideoPanel /> : null}
            {selectedMenuOption === "Fill" ? <FillPanel /> : null}
        </div>
    );
});