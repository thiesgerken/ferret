import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
};

// In production the Ingress routes these paths to the server. The static export can't rewrite,
// so only `next dev` proxies them.
if (process.env.NODE_ENV === "development") {
  const serverUrl = (process.env.FERRET_SERVER_URL?.trim() || "http://127.0.0.1:8080").replace(/\/$/, "");
  nextConfig.rewrites = async () => [
    { source: "/graphql", destination: `${serverUrl}/graphql` },
    { source: "/api/:path*", destination: `${serverUrl}/api/:path*` },
  ];
}

export default nextConfig;
