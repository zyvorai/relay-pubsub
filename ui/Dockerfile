FROM node:22-alpine AS builder
WORKDIR /app
COPY package.json tsconfig.json index.html ./
COPY src ./src
RUN npm install && npm run build

FROM nginx:1.27-alpine
COPY --from=builder /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
