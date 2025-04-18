"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import ExportVideoPanel from "./panels/ExportVideoPanel";
import AnimationsPanel from "./panels/AnimationsPanel";
import AudioResourcesPanel from "./panels/AudioResourcesPanel";
import AudioMixerPanel from "./panels/AudioMixerPanel";
import FillPanel from "./panels/FillPanel";
import ImageResourcesPanel from "./panels/ImageResourcesPanel";
import TextResourcesPanel from "./panels/TextResourcesPanel";
import VideoResourcesPanel from "./panels/VideoResourcesPanel";
import EffectsPanel from "./panels/EffectsPanel";
import MafsPanel from "./panels/MafsPanel";

const Resources = observer(() => {
    const state = React.useContext(StateContext);
    const selectedMenuOption = state.selectedMenuOption;
    return (
        <div className="bg-slate-200 dark:bg-gray-700 text-black dark:text-white h-full">
            {selectedMenuOption === "Videos" ? <VideoResourcesPanel /> : null}
            {selectedMenuOption === "Audios" ? <AudioResourcesPanel /> : null}
            {selectedMenuOption === "Audio Mixer" ? <AudioMixerPanel /> : null}
            {selectedMenuOption === "Images" ? <ImageResourcesPanel /> : null}
            {selectedMenuOption === "Text" ? <TextResourcesPanel /> : null}
            {selectedMenuOption === "Animations" ? <AnimationsPanel /> : null}
            {selectedMenuOption === "Effects" ? <EffectsPanel /> : null}
            {selectedMenuOption === "Export" ? <ExportVideoPanel /> : null}
            {selectedMenuOption === "Background Fill" ? <FillPanel /> : null}
            {selectedMenuOption === "Mathematical Objects" ? <MafsPanel /> : null}
        </div>
    );
});

export default Resources;