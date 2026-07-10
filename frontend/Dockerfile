# syntax=docker/dockerfile:1

FROM node:20-bookworm-slim

WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm ci || npm install

COPY . .

EXPOSE 3000

ENV HOST=0.0.0.0
ENV NUXT_HOST=0.0.0.0
ENV NUXT_PORT=3000

CMD ["npm", "run", "dev"]
