"use client";
import React, { createContext } from "react";
import { State } from "./state";

export const StateContext = createContext(new State());

export function StateProvider(props: { children: React.ReactNode }) {
  const [state] = React.useState(new State());
  return (
    <StateContext.Provider value={state}>
      {props.children}
    </StateContext.Provider>
  );
}
