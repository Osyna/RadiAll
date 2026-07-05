FROM nginx:alpine-slim

# Static site — copy files and configure an optimized nginx.
COPY index.html /usr/share/nginx/html/
COPY styles   /usr/share/nginx/html/styles
COPY scripts  /usr/share/nginx/html/scripts
COPY assets   /usr/share/nginx/html/assets

RUN rm -f /etc/nginx/conf.d/default.conf && \
    printf 'server {\n\
    listen 8080;\n\
    server_tokens off;\n\
    root /usr/share/nginx/html;\n\
    index index.html;\n\
\n\
    gzip on;\n\
    gzip_comp_level 6;\n\
    gzip_min_length 256;\n\
    gzip_vary on;\n\
    gzip_types text/plain text/css application/javascript image/svg+xml;\n\
\n\
    add_header X-Frame-Options "SAMEORIGIN";\n\
    add_header X-Content-Type-Options "nosniff";\n\
    add_header Referrer-Policy "strict-origin-when-cross-origin";\n\
\n\
    location ~* \\.(?:css|js|png|svg|woff2?)$ {\n\
        expires 30d;\n\
        add_header Cache-Control "public, immutable";\n\
    }\n\
    location / {\n\
        try_files $uri $uri/ /index.html;\n\
        expires 1h;\n\
    }\n\
}\n' > /etc/nginx/conf.d/optimized.conf

EXPOSE 8080
CMD ["nginx", "-g", "daemon off;"]
