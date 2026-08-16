import type { NextConfig } from "next";

if (process.env.DEMO_SCENARIO === "build-failure") {
  throw new Error("Injected demo build failure");
}

const nextConfig: NextConfig = {};

export default nextConfig;
