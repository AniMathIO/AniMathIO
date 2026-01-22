"use client";

import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import DashboardPanel from "./components/panels/DashboardPanel";
import Head from "next/head";
import AniMathIO from "../public/images/AniMathIO.png";

const Dashboard = observer(() => {
  const state = React.useContext(StateContext);

  // Only show dashboard if editor is not active
  if (state.isEditorActive) {
    return null;
  }

  return (
    <React.Fragment>
      <Head>
        <title>AniMathIO - Dashboard</title>
        <link rel="icon" href={AniMathIO.src} />
      </Head>

      <div className="bg-slate-200 dark:bg-gray-800 h-svh w-full">
        <DashboardPanel />
      </div>
    </React.Fragment>
  );
});

export default Dashboard;
