"use client";
import React, { createContext } from "react";
import { RootStore } from "./RootStore";

export const StateContext = createContext(new RootStore());

export function StateProvider(props: { children: React.ReactNode }) {
  const [state] = React.useState(() => new RootStore());
  return (
    <StateContext.Provider value={state}>
      {props.children}
    </StateContext.Provider>
  );
}
