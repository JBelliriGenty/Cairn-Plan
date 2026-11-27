# cairn-plan-agent: Claude Code, git and the small program that drives them.
FROM debian:bookworm-slim
# Pinned: a new Claude Code is a change to review, not a surprise.
ARG CLAUDE_VERSION=2.1.303
# Debian packages the checks of your projects need and only root can install (a JDK, a C compiler).
# The worker installs the rest in its home, kept from one ticket to the next.
ARG EXTRA_PACKAGES=""
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git python3 tzdata ${EXTRA_PACKAGES} \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 1000 worker \
 # the worker clones the clone this program made as root
 && git config --system --add safe.directory /work/src \
 && git config --system --add safe.directory /work/src/.git
# Claude Code belongs to the user it runs as (official installer).
USER worker
RUN curl -fsSL https://claude.ai/install.sh | bash -s ${CLAUDE_VERSION}
USER root
COPY agent.py prompt.md prompt-chef.md prompt-avis.md rules.json team.json test_agent.py /opt/agent/
RUN cd /opt/agent && python3 test_agent.py
# /tmp goes to the work volume, like the home of the worker (agent.py makes both): what a ticket
# installs or builds is not written in the container. Last step: a build step needs the real /tmp.
RUN rm -rf /tmp && ln -s /work/w/tmp /tmp
VOLUME /work
EXPOSE 9095
CMD ["python3", "/opt/agent/agent.py"]
