// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/main.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Frontend entry point.
// =============================================================================
import { mount } from "svelte";
import "uplot/dist/uPlot.min.css";
import "./app.css";
import App from "./App.svelte";
import RegionPicker from "./pages/RegionPicker.svelte";

// The recorder's region picker runs in its own window (index.html#region).
const target = document.getElementById("app")!;
const app = location.hash.startsWith("#region") ? mount(RegionPicker, { target }) : mount(App, { target });

export default app;
