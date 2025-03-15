"use client";
import React, { useEffect, useState } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { AudioEditorElement } from "@/types";
import { SpeakerWaveIcon } from "@heroicons/react/24/solid";
import AudioTrack from "../entity/AudioTrack";


const AudioMixerPanel = observer(() => {
    const state = React.useContext(StateContext);

    // Filter for audio elements that are active at the current time
    const currentTime = state.currentTimeInMs;
    const activeAudioElements = state.editorElements
        .filter((element): element is AudioEditorElement =>
            element.type === "audio" &&
            element.timeFrame.start <= currentTime &&
            element.timeFrame.end >= currentTime
        );

    // Master volume control
    const [masterVolume, setMasterVolume] = useState(100);

    const handleMasterVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const newVolume = parseInt(e.target.value);
        setMasterVolume(newVolume);

        // Apply to all audio elements
        activeAudioElements.forEach(element => {
            const audioElement = document.getElementById(element.properties.elementId) as HTMLAudioElement;
            if (audioElement) {
                // Apply proportionally based on individual track volume
                const individualVolume = element.properties.volume || 1;
                audioElement.volume = (newVolume / 100) * individualVolume;

                // Update master volume for each element
                state.updateAudioSettings(element.id, { masterVolume: newVolume / 100 });
            }
        });
    };

    // Initialize master volume on component mount and when elements change
    useEffect(() => {
        // Check if we have a master volume setting stored
        const firstElement = activeAudioElements[0];
        if (firstElement?.properties.masterVolume !== undefined) {
            setMasterVolume(firstElement.properties.masterVolume * 100);
        }
    }, [activeAudioElements]);

    return (
        <div className="bg-slate-200 dark:bg-gray-700">
            <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
                Audio Mixer
            </div>

            {activeAudioElements.length === 0 ? (
                <div className="p-4 text-center text-gray-400">
                    No active audio tracks at current position
                </div>
            ) : (
                <>
                    <div className="p-2 border-b border-gray-600 dark:bg-gray-800">
                        <div className="text-sm font-semibold mb-1">Master Volume</div>
                        <div className="flex items-center">
                            <SpeakerWaveIcon className="mr-2 w-6 h-6" />
                            <input
                                aria-label="Master Volume"
                                type="range"
                                min="0"
                                max="100"
                                value={masterVolume}
                                onChange={handleMasterVolumeChange}
                                className="flex-grow h-2 rounded-lg appearance-none bg-gray-600 cursor-pointer"
                            />
                            <span className="ml-2 text-xs w-8 text-right">{masterVolume}%</span>
                        </div>
                    </div>

                    <div className="max-h-[300px] overflow-y-auto">
                        {activeAudioElements.map((element, index) => (
                            <AudioTrack key={element.id} element={element} index={index} />
                        ))}
                    </div>
                </>
            )}
        </div>
    );
});

export default AudioMixerPanel;