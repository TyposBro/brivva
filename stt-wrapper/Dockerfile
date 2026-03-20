FROM python:3.11-slim

WORKDIR /app

RUN pip install --no-cache-dir websockets==15.0.1 numpy

COPY server.py .

EXPOSE 8766

CMD ["python", "server.py"]
