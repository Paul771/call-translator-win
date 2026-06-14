defmodule Translator.CommandServer do
  @moduledoc "Tiny TCP server on port 5051 for commands from the web UI."

  use GenServer
  require Logger

  @port 5051

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    listen = try_listen(10)
    Logger.info("CommandServer listening on port #{@port}")
    spawn_link(fn -> accept_loop(listen) end)
    {:ok, %{listen: listen}}
  end

  defp try_listen(0), do: raise("Failed to listen on port #{@port} after retries")
  defp try_listen(n) do
    case :gen_tcp.listen(@port, [:binary, packet: :line, active: false, reuseaddr: true]) do
      {:ok, listen} -> listen
      {:error, :eaddrinuse} ->
        Logger.warning("Port #{@port} in use, attempting cleanup (#{n-1} left)...")
        kill_port_owner(@port)
        :timer.sleep(1000)
        try_listen(n - 1)
    end
  end

  defp kill_port_owner(port) do
    try do
      case System.cmd("netstat", ["-ano"], stderr_to_stdout: true) do
        {output, 0} ->
          output
          |> String.split("\n")
          |> Enum.find(fn line -> String.contains?(line, ":#{port}") && String.contains?(line, "LISTENING") end)
          |> case do
            nil -> :ok
            line ->
              line |> String.split() |> List.last() |> String.trim() |> case do
                "" -> :ok
                pid_str ->
                  case Integer.parse(pid_str) do
                    {pid, _} ->
                      Logger.warning("Killing orphaned process PID=#{pid} on port #{port}")
                      System.cmd("taskkill", ["/F", "/PID", "#{pid}"], stderr_to_stdout: true)
                      :timer.sleep(500)
                    _ -> :ok
                  end
              end
          end
        _ -> :ok
      end
    rescue
      _ -> :ok
    end
  end

  defp accept_loop(listen) do
    case :gen_tcp.accept(listen) do
      {:ok, socket} ->
        case :gen_tcp.recv(socket, 0, 5000) do
          {:ok, data} ->
            resp = handle_command(String.trim(data))
            :gen_tcp.send(socket, resp <> "\n")

          _ ->
            :ok
        end

        :gen_tcp.close(socket)
        accept_loop(listen)

      {:error, reason} ->
        Logger.error("CommandServer accept error: #{inspect(reason)}")
    end
  end

  defp handle_command("start") do
    Translator.AudioEngine.start_pipelines()
    "ok"
  end

  defp handle_command("stop") do
    Translator.AudioEngine.stop_pipelines()
    "ok"
  end

  defp handle_command("mute_outgoing") do
    Translator.AudioEngine.set_config(:mute_outgoing, true)
    "ok"
  end

  defp handle_command("unmute_outgoing") do
    Translator.AudioEngine.set_config(:mute_outgoing, false)
    "ok"
  end

  defp handle_command("mute_incoming") do
    Translator.AudioEngine.set_config(:mute_incoming, true)
    "ok"
  end

  defp handle_command("unmute_incoming") do
    Translator.AudioEngine.set_config(:mute_incoming, false)
    "ok"
  end

  defp handle_command("preview:" <> rest) do
    case String.split(rest, ":", parts: 2) do
      [lang, voice] ->
        Translator.AudioEngine.send_command(%{
          "cmd" => "tts_preview",
          "lang" => lang,
          "voice" => voice
        })
        "ok:previewing"

      _ ->
        "error:bad_preview_format"
    end
  end

  defp handle_command("list_devices") do
    Translator.AudioEngine.send_command(%{"cmd" => "list_devices"})
    "ok:listing"
  end

  defp handle_command("poll_audio") do
    items = Translator.AudioEngine.pop_audio()
    Jason.encode!(items)
  end

  defp handle_command("restart") do
    Translator.AudioEngine.restart_engine_async()
    "ok:restarting"
  end

  defp handle_command("status") do
    %{status: status} = Translator.AudioEngine.status()
    "ok:#{status}"
  end

  defp handle_command(other) do
    Logger.warning("Unknown command: #{other}")
    "error:unknown_command"
  end
end
