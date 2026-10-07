defmodule Redoubt.Shell.MixProject do
  use Mix.Project

  def project do
    [
      app: :redoubt_shell,
      version: "0.1.0",
      elixir: "~> 1.20",
      deps: [],
      # The wire's Elixir codecs and their generated clients (docs/servers/wire.md, "Generated
      # clients") are part of the shell's application: a session binds every server through them.
      elixirc_paths: ["lib" | Enum.map(~w(lib proto client), &Path.expand("../../libs/wire/elixir/" <> &1, __DIR__))],
      # beamlet looks a module up in the system's code before any other directory, so a
      # protocol consolidated here would never be the one used on Redoubt. Build as it runs.
      consolidate_protocols: false
    ]
  end

  # crypto for checksum: its natives are beamlet-crypto's on beamlet.
  def application, do: [extra_applications: [:crypto]]
end
