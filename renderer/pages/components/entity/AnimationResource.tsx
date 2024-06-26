"use client";
import React from "react";
import { StateContext } from "@/states";
// import { formatTimeToMinSec } from "@/utils";
import { observer } from "mobx-react";
import {
  TrashIcon
} from "@heroicons/react/24/solid";
import {
  Animation,
  FadeInAnimation,
  FadeOutAnimation,
  SlideDirection,
  SlideInAnimation,
  SlideOutAnimation,
  SlideTextType,
} from "@/types";

import dynamic from "next/dynamic";

const ANIMATION_TYPE_TO_LABEL: Record<string, string> = {
  fadeIn: "Fade In",
  fadeOut: "Fade Out",
  slideIn: "Slide In",
  slideOut: "Slide Out",
  breath: "Breath",
};
export type AnimationResourceProps = {
  animation: Animation;
};
const AnimationResource = observer((props: AnimationResourceProps) => {
  const state = React.useContext(StateContext);
  return (
    <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative min-h-[100px] p-2">
      <div className="flex flex-row justify-between w-full">
        <div className="text-white py-1 text-base text-left w-full">
          {ANIMATION_TYPE_TO_LABEL[props.animation.type]}
        </div>
        <button
          aria-label="Delete"
          className="hover:bg-[#00a0f5] bg-[rgba(0,0,0,.25)] rounded z-10 text-white font-bold py-1 text-lg"
          onClick={() => state.removeAnimation(props.animation.id)}
        >
          <TrashIcon className="w-6 h-6" />
        </button>
      </div>
      {props.animation.type === "fadeIn" ||
        props.animation.type === "fadeOut" ? (
        <FadeAnimation
          animation={props.animation as FadeInAnimation | FadeOutAnimation}
        />
      ) : null}
      {props.animation.type === "slideIn" ||
        props.animation.type === "slideOut" ? (
        <SlideAnimation
          animation={props.animation as SlideInAnimation | SlideOutAnimation}
        />
      ) : null}
    </div>
  );
});

export const FadeAnimation = observer(
  (props: { animation: FadeInAnimation | FadeOutAnimation }) => {
    const state = React.useContext(StateContext);
    return (
      <div className="flex flex-col w-full items-start">
        {/* duration */}
        <div className="flex flex-row items-center justify-between my-1">
          <div className="text-white text-xs">Duration (s):</div>
          <label className="text-white text-xs" htmlFor="duration" />

          <input
            id="duration"
            className="bg-slate-100 text-black rounded-lg px-2 py-1 ml-2 w-16 text-xs"
            type="number"
            value={props.animation.duration / 1000}
            onChange={(e) => {
              const duration = Number(e.target.value) * 1000;
              const isValidDuration = duration > 0;
              let newDuration = isValidDuration ? duration : 0;
              if (newDuration < 10) {
                newDuration = 10;
              }
              state.updateAnimation(props.animation.id, {
                ...props.animation,
                duration: newDuration,
              });
            }}
            placeholder="Duration in seconds"
          />
        </div>
      </div>
    );
  }
);

// Animation has direction 'left', 'right', 'top', 'bottom' in properties
// These properties can be selected by select element
export const SlideAnimation = observer(
  (props: { animation: SlideInAnimation | SlideOutAnimation }) => {
    const state = React.useContext(StateContext);
    return (
      <div className="flex flex-col w-full items-start">
        {/* duration */}
        <div className="flex flex-row items-center justify-between my-1">
          <label className="text-white text-xs" htmlFor="duration" >
            Duration (s):
          </label>
          <input
            id="duration"
            className="bg-slate-100 text-black rounded-lg px-2 py-1 ml-2 w-16 text-xs"
            type="number"
            value={props.animation.duration / 1000}
            onChange={(e) => {
              const duration = Number(e.target.value) * 1000;
              const isValidDuration = duration > 0;
              let newDuration = isValidDuration ? duration : 0;
              if (newDuration < 10) {
                newDuration = 10;
              }
              state.updateAnimation(props.animation.id, {
                ...props.animation,
                duration: newDuration,
              });
            }}
          />
        </div>
        <div className="flex flex-row items-center justify-between my-1">
          <div className="text-white text-xs">Direction</div>
          <select
            aria-label="Direction"
            className="bg-slate-100 text-black rounded-lg px-2 py-1 ml-2 w-16 text-xs"
            value={props.animation.properties.direction}
            onChange={(e) => {
              state.updateAnimation(props.animation.id, {
                ...props.animation,
                properties: {
                  ...props.animation.properties,
                  direction: e.target.value as SlideDirection,
                },
              });
            }}
          >
            <option value="left">Left</option>
            <option value="right">Right</option>
            <option value="top">Top</option>
            <option value="bottom">Bottom</option>
          </select>
        </div>
        <div className="flex flex-row items-center justify-between my-1">
          <div className="text-white text-xs">Use Mask</div>
          <input
            aria-label="Use Mask"
            className="bg-slate-100 text-black rounded-lg px-2 py-1 ml-2 w-16 text-xs"
            type="checkbox"
            checked={props.animation.properties.useClipPath}
            onChange={(e) => {
              state.updateAnimation(props.animation.id, {
                ...props.animation,
                properties: {
                  ...props.animation.properties,
                  useClipPath: e.target.checked,
                },
              });
            }}
          />
        </div>
        <div className="flex flex-row items-center justify-between my-1">
          <div className="text-white text-xs">Type</div>
          <select
            aria-label="Type"
            className="bg-slate-100 text-black rounded-lg px-2 py-1 ml-2 w-16 text-xs"
            value={props.animation.properties.textType}
            onChange={(e) => {
              state.updateAnimation(props.animation.id, {
                ...props.animation,
                properties: {
                  ...props.animation.properties,
                  textType: e.target.value as SlideTextType,
                },
              });
            }}
          >
            <option value="none">None</option>
            <option value="character">Character</option>
          </select>
        </div>
      </div>
    );
  }
);

export default dynamic(() => Promise.resolve(AnimationResource), { ssr: false });