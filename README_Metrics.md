# Metrics

In order to run metrics locally, you need to run a prometheus instance to collect your raiko host metrics.

This can be done by running using the [docker-compose.yml](./docker/docker-compose.yml) file with the following
params:

```bash
docker compose up -d
```

or the following if you are running Raiko outside of docker.

```bash
docker compose up -d --scale raiko=0,pccs=0
```

To view the Raiko Metrics dashboard you can load the grafana UI in your browser by going to `localhost:3000`.
You will be asked to log in using the default authentication - in this case the username is `admin` and the
password is also `admin` (this can be changed to match your preference).

You can now simply click on the `+` icon in the upper right corner of the UI to import a dashboard.
In the dashboard import screen you have the option to upload a dashboard JSON file.
This repo contains the [raiko.json](./docker/monitoring/grafana/raiko.json) dashboard definition file.
Next, you will be asked to provide a prometheus data source that provides the Raiko metrics.
If you have your own setup, please use that prometheus instance, otherwise, a prometheus instance will be available
on the following URL `http://prometheus:9090` (docker host definition).

**NOTE:** If you have an existing grafana instance running, it might not have access to `http://prometheus:9090` so
the prometheus instance will be available at `http://localhost:9090`. If you want to add Raiko metrics to your existing
prometheus server however - check out the [prometheus.yml](./docker/monitoring/prometheus/prometheus.yml) file.
