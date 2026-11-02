import http from "http";

const server = http.createServer((req, res) => {
  if (req.url === "/health" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({
      status: "UP",
      protocol: "steve-agent-arena",
      timestamp: new Date().toISOString(),
      network: "solana-devnet"
    }));
  } else {
    res.writeHead(404);
    res.end();
  }
});

server.listen(8080, () => {
  console.log("[HEALTH] Liveness probe listening on port 8080");
});