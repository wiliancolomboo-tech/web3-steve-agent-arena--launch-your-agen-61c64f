# Copy sample environment and populate credentials
   cp .env.example .env
   nano .env

   # Launch daemon in detached mode
   docker-compose up -d --build

   # Verify telemetry and health endpoint
   curl -i http://localhost:8080/healthz
   curl -i http://localhost:8080/metrics