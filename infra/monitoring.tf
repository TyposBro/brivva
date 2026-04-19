# CloudWatch dashboards + alarms for the Fargate media service.
#
# What it watches:
#   - ECS service health (CPU, memory, running-task count)
#   - Media pipeline lifecycle events extracted from server-rs JSON logs
#     (ffmpeg crashes, restart exhaustion, workers API failures)
#
# Wire-up:
#   - SNS topic receives every alarm; subscribe an email via alarm_email var.
#   - Metric filters emit counts into the "Brivva" custom namespace so alarms
#     can page on log events without needing app-side StatsD.

locals {
  alarm_namespace = "Brivva"
  alarm_enabled   = var.alarm_enabled
}

# ── SNS topic (alarm sink) ─────────────────────────────────

resource "aws_sns_topic" "alarms" {
  count = local.alarm_enabled ? 1 : 0
  name  = "${var.project}-alarms"
}

resource "aws_sns_topic_subscription" "alarm_email" {
  count     = local.alarm_enabled && var.alarm_email != "" ? 1 : 0
  topic_arn = aws_sns_topic.alarms[0].arn
  protocol  = "email"
  endpoint  = var.alarm_email
}

# ── Log-based metric filters ───────────────────────────────
# server-rs emits structured JSON via tracing_subscriber::fmt().json().
# Shape: { "timestamp": "...", "level": "...", "fields": { "message": "..." } }
# Patterns select the "message" field — the macro's first positional arg.

resource "aws_cloudwatch_log_metric_filter" "ffmpeg_crashed" {
  name           = "${var.project}-ffmpeg-crashed"
  log_group_name = aws_cloudwatch_log_group.app.name
  # The warn-level crash+restart event from ffmpeg.rs health monitor.
  pattern = "{ $.fields.message = \"*ffmpeg rtmp process crashed*\" }"

  metric_transformation {
    name          = "FfmpegCrashCount"
    namespace     = local.alarm_namespace
    value         = "1"
    default_value = "0"
    unit          = "Count"
  }
}

resource "aws_cloudwatch_log_metric_filter" "ffmpeg_gave_up" {
  name           = "${var.project}-ffmpeg-gave-up"
  log_group_name = aws_cloudwatch_log_group.app.name
  # Error-level event from ffmpeg.rs:332 once all restart attempts are exhausted.
  pattern = "{ $.fields.message = \"*ffmpeg rtmp giving up*\" }"

  metric_transformation {
    name          = "FfmpegGiveUpCount"
    namespace     = local.alarm_namespace
    value         = "1"
    default_value = "0"
    unit          = "Count"
  }
}

resource "aws_cloudwatch_log_metric_filter" "workers_status_failed" {
  name           = "${var.project}-workers-status-failed"
  log_group_name = aws_cloudwatch_log_group.app.name
  pattern        = "{ $.fields.message = \"*workers status*update failed*\" }"

  metric_transformation {
    name          = "WorkersStatusUpdateFailures"
    namespace     = local.alarm_namespace
    value         = "1"
    default_value = "0"
    unit          = "Count"
  }
}

resource "aws_cloudwatch_log_metric_filter" "ws_auth_rejected" {
  name           = "${var.project}-ws-auth-rejected"
  log_group_name = aws_cloudwatch_log_group.app.name
  pattern        = "{ $.fields.message = \"*ws host upgrade rejected*\" }"

  metric_transformation {
    name          = "WsAuthRejections"
    namespace     = local.alarm_namespace
    value         = "1"
    default_value = "0"
    unit          = "Count"
  }
}

resource "aws_cloudwatch_log_metric_filter" "session_started" {
  name           = "${var.project}-session-started"
  log_group_name = aws_cloudwatch_log_group.app.name
  pattern        = "{ $.fields.message = \"*ffmpeg rtmp streams started*\" }"

  metric_transformation {
    name          = "LiveSessionStarts"
    namespace     = local.alarm_namespace
    value         = "1"
    default_value = "0"
    unit          = "Count"
  }
}

# ── Alarms ─────────────────────────────────────────────────

resource "aws_cloudwatch_metric_alarm" "ecs_task_count" {
  count               = local.alarm_enabled ? 1 : 0
  alarm_name          = "${var.project}-ecs-running-tasks-low"
  alarm_description   = "Fargate service has fewer running tasks than desired. Tunnel users will see 530/1033."
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 2
  threshold           = 1
  treat_missing_data  = "breaching"

  metric_name = "RunningTaskCount"
  namespace   = "ECS/ContainerInsights"
  period      = 60
  statistic   = "Minimum"

  dimensions = {
    ClusterName = aws_ecs_cluster.app.name
    ServiceName = aws_ecs_service.app.name
  }

  alarm_actions = [aws_sns_topic.alarms[0].arn]
  ok_actions    = [aws_sns_topic.alarms[0].arn]
}

resource "aws_cloudwatch_metric_alarm" "ecs_cpu_high" {
  count               = local.alarm_enabled ? 1 : 0
  alarm_name          = "${var.project}-ecs-cpu-high"
  alarm_description   = "CPU above 80% for 15 minutes. Bump task_cpu or scale horizontally."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 3
  threshold           = 80

  metric_name = "CPUUtilization"
  namespace   = "AWS/ECS"
  period      = 300
  statistic   = "Average"

  dimensions = {
    ClusterName = aws_ecs_cluster.app.name
    ServiceName = aws_ecs_service.app.name
  }

  alarm_actions = [aws_sns_topic.alarms[0].arn]
}

resource "aws_cloudwatch_metric_alarm" "ecs_memory_high" {
  count               = local.alarm_enabled ? 1 : 0
  alarm_name          = "${var.project}-ecs-memory-high"
  alarm_description   = "Memory above 85% for 15 minutes. OOM-kill imminent. Bump task_memory."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 3
  threshold           = 85

  metric_name = "MemoryUtilization"
  namespace   = "AWS/ECS"
  period      = 300
  statistic   = "Average"

  dimensions = {
    ClusterName = aws_ecs_cluster.app.name
    ServiceName = aws_ecs_service.app.name
  }

  alarm_actions = [aws_sns_topic.alarms[0].arn]
}

resource "aws_cloudwatch_metric_alarm" "ffmpeg_crash_rate" {
  count               = local.alarm_enabled ? 1 : 0
  alarm_name          = "${var.project}-ffmpeg-crash-rate"
  alarm_description   = "More than 3 ffmpeg rtmp crashes in 5 minutes. Check destination health + CPU saturation."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  threshold           = 3
  treat_missing_data  = "notBreaching"

  metric_name = "FfmpegCrashCount"
  namespace   = local.alarm_namespace
  period      = 300
  statistic   = "Sum"

  alarm_actions = [aws_sns_topic.alarms[0].arn]
}

resource "aws_cloudwatch_metric_alarm" "ffmpeg_gave_up" {
  count               = local.alarm_enabled ? 1 : 0
  alarm_name          = "${var.project}-ffmpeg-gave-up"
  alarm_description   = "FFmpeg exhausted restart attempts. Stream is DOWN until session restarts. Page immediately."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  threshold           = 0
  treat_missing_data  = "notBreaching"

  metric_name = "FfmpegGiveUpCount"
  namespace   = local.alarm_namespace
  period      = 60
  statistic   = "Sum"

  alarm_actions = [aws_sns_topic.alarms[0].arn]
}

# ── Dashboard ──────────────────────────────────────────────

resource "aws_cloudwatch_dashboard" "app" {
  dashboard_name = "${var.project}-ops"

  dashboard_body = jsonencode({
    widgets = [
      {
        type   = "metric"
        x      = 0
        y      = 0
        width  = 12
        height = 6
        properties = {
          title   = "ECS CPU / Memory %"
          region  = var.region
          view    = "timeSeries"
          stacked = false
          metrics = [
            ["AWS/ECS", "CPUUtilization", "ServiceName", aws_ecs_service.app.name, "ClusterName", aws_ecs_cluster.app.name, { label = "CPU %" }],
            [".", "MemoryUtilization", ".", ".", ".", ".", { label = "Mem %" }],
          ]
          yAxis = { left = { min = 0, max = 100 } }
        }
      },
      {
        type   = "metric"
        x      = 12
        y      = 0
        width  = 12
        height = 6
        properties = {
          title   = "ECS task count"
          region  = var.region
          view    = "timeSeries"
          stacked = false
          metrics = [
            ["ECS/ContainerInsights", "RunningTaskCount", "ServiceName", aws_ecs_service.app.name, "ClusterName", aws_ecs_cluster.app.name, { label = "Running" }],
            [".", "DesiredTaskCount", ".", ".", ".", ".", { label = "Desired" }],
          ]
        }
      },
      {
        type   = "metric"
        x      = 0
        y      = 6
        width  = 12
        height = 6
        properties = {
          title   = "FFmpeg crashes + restart exhaustion"
          region  = var.region
          view    = "timeSeries"
          stacked = false
          metrics = [
            [local.alarm_namespace, "FfmpegCrashCount", { label = "Crashes" }],
            [".", "FfmpegGiveUpCount", { label = "Gave up" }],
          ]
        }
      },
      {
        type   = "metric"
        x      = 12
        y      = 6
        width  = 12
        height = 6
        properties = {
          title   = "Pipeline events"
          region  = var.region
          view    = "timeSeries"
          stacked = false
          metrics = [
            [local.alarm_namespace, "LiveSessionStarts", { label = "Sessions started" }],
            [".", "WsAuthRejections", { label = "WS auth rejected" }],
            [".", "WorkersStatusUpdateFailures", { label = "Workers status fail" }],
          ]
        }
      },
      {
        type   = "log"
        x      = 0
        y      = 12
        width  = 24
        height = 8
        properties = {
          title  = "Recent errors and warnings"
          region = var.region
          query  = "SOURCE '${aws_cloudwatch_log_group.app.name}' | fields @timestamp, level, fields.message, fields.stream_id, fields.lang\n| filter level in [\"ERROR\", \"WARN\"]\n| sort @timestamp desc\n| limit 50"
        }
      },
    ]
  })
}
