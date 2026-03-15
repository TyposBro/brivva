FROM python:3.11-slim

WORKDIR /app

RUN pip install --no-cache-dir websockets==14.2

COPY server.py .

EXPOSE 8766

CMD ["python", "server.py"]
