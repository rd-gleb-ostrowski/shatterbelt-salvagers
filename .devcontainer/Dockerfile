FROM mcr.microsoft.com/devcontainers/typescript-node

ARG TZ
ARG GIT_DELTA_VERSION=0.19.2
ENV TZ="$TZ"

ARG COPILOT_VERSION=latest

# Install basic development tools and iptables/ipset
RUN apt-get update && apt-get install -y --no-install-recommends \
  less \
  git \
  procps \
  sudo \
  fzf \
  zsh \
  man-db \
  unzip \
  gnupg2 \
  gh \
  iptables \
  ipset \
  iproute2 \
  dnsutils \
  aggregate \
  jq \
  nano \
  neovim \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

# Ensure default node user has access to /usr/local/share
RUN mkdir -p /usr/local/share/npm-global && \
  chown -R node:node /usr/local/share

ARG USERNAME=node

# Persist bash history.
RUN SNIPPET="export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
  && mkdir /commandhistory \
  && touch /commandhistory/.bash_history \
  && chown -R $USERNAME /commandhistory

# Set `DEVCONTAINER` environment variable to help with orientation
ENV DEVCONTAINER=true

# Create workspace and config directories and set permissions
RUN mkdir -p /workspace /home/node/.claude /home/node/.copilot && \
  chown -R node:node /workspace /home/node/.claude /home/node/.copilot

WORKDIR /workspace

# Set up non-root user
USER node

# Set the default editor and visual
ENV EDITOR=nano
ENV VISUAL=nano

RUN sh -c "echo \"export PROMPT_COMMAND='history -a'\" >> /home/node/.zshrc"
RUN sh -c "echo export HISTFILE=/commandhistory/.bash_history >> /home/node/.zshrc"

USER root
ENV GIT_DELTA_VERSION="$GIT_DELTA_VERSION"
RUN ARCH=$(dpkg --print-architecture) && \
  wget "https://github.com/dandavison/delta/releases/download/${GIT_DELTA_VERSION}/git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  sudo dpkg -i "git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  rm "git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb"

USER node
RUN sh -c "curl -LsSf https://astral.sh/uv/install.sh | sh"
RUN sh -c "echo 'eval \"$(uvx --generate-shell-completion zsh)\"' >> ~/.zshrc"
RUN sh -c "echo 'eval \"$(uv generate-shell-completion zsh)\"' >> ~/.zshrc"
# Install Copilot CLI
# RUN npm install -g @github/copilot@${COPILOT_VERSION}
# USER root
# # Copy and set up firewall script
# COPY init-firewall.sh /usr/local/bin/
# COPY allowed-domains.txt /tmp/
# RUN chmod +x /usr/local/bin/init-firewall.sh && \
#   echo "node ALL=(root) NOPASSWD: /usr/local/bin/init-firewall.sh" > /etc/sudoers.d/node-firewall && \
#   chmod 0440 /etc/sudoers.d/node-firewall
# USER node

# TODO
# serena + config mount
