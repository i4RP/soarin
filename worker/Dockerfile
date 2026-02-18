FROM node:20-slim

RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    python3 \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code

RUN useradd -m -s /bin/bash worker
USER worker
WORKDIR /home/worker

COPY --chown=worker:worker bridge/ /home/worker/bridge/

WORKDIR /home/worker/bridge
RUN npm install

ENV NODE_ENV=production
EXPOSE 3000

CMD ["node", "server.js"]
