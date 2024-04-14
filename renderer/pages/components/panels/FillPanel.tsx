"use client";
import React from "react";
import { StateContext } from "../../states";
import { observer } from "mobx-react";
import { GithubPicker, SwatchesPicker, TwitterPicker } from "react-color";

const professionalVideoColors = [
  "#000000", // Black
  "#333333", // Very Dark Gray
  "#666666", // Dark Gray
  "#999999", // Gray
  "#CCCCCC", // Light Gray
  "#FFFFFF", // White

  "#000033", // Very Dark Blue
  "#003366", // Dark Blue
  "#336699", // Medium Blue
  "#6699CC", // Blue
  "#99CCFF", // Light Blue
  "#CCE5FF", // Very Light Blue

  "#003300", // Very Dark Green
  "#006600", // Dark Green
  "#339933", // Green
  "#66CC99", // Light Green
  "#99FFCC", // Very Light Green
  "#CCFFCC", // Pale Green

  "#330000", // Very Dark Brown
  "#663300", // Dark Brown
  "#996633", // Brown
  "#CC9966", // Light Brown
  "#FFCC99", // Very Light Brown
  "#FFE5CC", // Pale Brown

  "#660000", // Very Dark Red
  "#990000", // Dark Red
  "#CC3333", // Red
  "#FF6666", // Light Red
  "#FF9999", // Very Light Red
  "#FFCCCC", // Pale Red

  "#666600", // Dark Yellow
  "#999900", // Olive
  "#CCCC00", // Yellow
  "#FFFF00", // Bright Yellow
  "#FFFF66", // Light Yellow
  "#FFFFCC"  // Pale Yellow
];

export const FillPanel = observer(() => {
  const state = React.useContext(StateContext);
  // Color Picker
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Background Colour Fill
      </div>
      <div className="flex items-center justify-center ">
        <TwitterPicker
          width="250px"
          colors={professionalVideoColors}
          color={state.backgroundColor}
          onChangeComplete={(color: any) => {
            // console.log(color);
            state.setBackgroundColor(color.hex);
          }}
        ></TwitterPicker>
      </div >
    </>
  );
});
