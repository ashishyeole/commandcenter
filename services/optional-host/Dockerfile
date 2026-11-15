FROM python:3.13-slim
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 PCC_BIND=0.0.0.0 PORT=8080
RUN groupadd --gid 10001 pcc && useradd --uid 10001 --gid pcc --no-create-home pcc && mkdir /data && chown pcc:pcc /data
WORKDIR /app
COPY requirements-push.lock ./
RUN pip install --no-cache-dir -r requirements-push.lock
COPY --chown=pcc:pcc server.py storage.py worker.py marketplace.py push.py ./
USER 10001:10001
EXPOSE 8080
CMD ["python", "server.py"]
