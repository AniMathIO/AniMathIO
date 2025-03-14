"use client";
import React, { useEffect, useState } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { AudioEditorElement } from "@/types";
import { SpeakerWaveIcon, SpeakerXMarkIcon } from "@heroicons/react/24/solid";

interface AudioTrackProps {
    element: AudioEditorElement;
    index: number;
}

const AudioTrack = observer(({ element, index }: AudioTrackProps) => {
    const state = React.useContext(StateContext);
    const [volume, setVolume] = useState(100);
    const [muted, setMuted] = useState(false);
    const audioElement = document.getElementById(element.properties.elementId) as HTMLAudioElement;

    useEffect(() => {
        if (audioElement) {
            // Initialize volume and mute state from properties or audio element
            const volumeValue = element.properties.volume !== undefined ?
                element.properties.volume * 100 : 100;
            setVolume(volumeValue);

            const mutedValue = element.properties.muted !== undefined ?
                element.properties.muted : false;
            setMuted(mutedValue);

            // Apply stored settings to audio element
            audioElement.volume = volumeValue / 100;
            audioElement.muted = mutedValue;
        }
    }, [audioElement, element.id]);

    const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const newVolume = parseInt(e.target.value);
        setVolume(newVolume);

        if (audioElement) {
            audioElement.volume = newVolume / 100;

            // Use the state's updateAudioSettings method
            state.updateAudioSettings(element.id, { volume: newVolume / 100 });
        }
    };

    const handleMuteToggle = () => {
        if (audioElement) {
            const newMuted = !audioElement.muted;
            audioElement.muted = newMuted;
            setMuted(newMuted);

            // Use the state's updateAudioSettings method
            state.updateAudioSettings(element.id, { muted: newMuted });
        }
    };

    return (
        <div className="flex items-center p-2 border-b border-gray-600">
            <div className="flex flex-row gap-1 items-center mr-3">
                <span className="text-black dark:text-white text-sm font-bold mr-1">{index + 1}</span>
                <span className="border-l border-gray-600 h-12"></span>
            </div>
            <div className="flex-grow">
                <div className="truncate text-sm">{element.name}</div>
                <div className="flex items-center mt-1">
                    <button
                        onClick={handleMuteToggle}
                        className={`w-6 h-6 mr-2 flex items-center justify-center rounded-xl ${muted ? 'bg-red-500' : 'bg-green-500'}`}
                    >
                        {muted ? (
                            <SpeakerXMarkIcon className="w-4 h-4" />
                        ) : (
                            <SpeakerWaveIcon className="w-4 h-4" />
                        )}
                    </button>
                    <input
                        aria-label="Volume"
                        type="range"
                        min="0"
                        max="100"
                        value={volume}
                        onChange={handleVolumeChange}
                        className="flex-grow h-2 rounded-lg appearance-none bg-gray-600 cursor-pointer"
                    />
                    <span className="ml-2 text-xs w-8 text-right">{volume}%</span>
                </div>
            </div>
        </div>
    );
});


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