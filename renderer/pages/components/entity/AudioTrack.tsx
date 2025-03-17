"use client";
import React, { useEffect, useState } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { AudioEditorElement } from "@/types";
import { SpeakerWaveIcon, SpeakerXMarkIcon } from "@heroicons/react/24/solid";
import dynamic from "next/dynamic";


interface AudioTrackProps {
    element: AudioEditorElement;
    index: number;
}

const AudioTrack = observer(({ element, index }: AudioTrackProps) => {
    const state = React.useContext(StateContext);
    const [volume, setVolume] = useState(100);
    const [muted, setMuted] = useState(false);
    const audioElement = document?.getElementById(element.properties.elementId) as HTMLAudioElement;

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

export default dynamic(() => Promise.resolve(AudioTrack), { ssr: false });