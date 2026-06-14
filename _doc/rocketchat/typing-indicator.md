# Typing Indicator — RocketChat DDP Protocol

## How the RocketChat web client sends typing notifications

Source: [Rocket.Chat UserAction.ts](https://github.com/RocketChat/Rocket.Chat/blob/develop/apps/meteor/app/ui/client/lib/UserAction.ts)

The web client does **NOT** send a `typing` event with a boolean. Instead, it uses the unified
**`user-activity`** event with an **array** of active activity types.

### Sending (publish / method call)

```json
{
  "msg": "method",
  "method": "stream-notify-room",
  "id": "...",
  "params": [
    "ROOM_ID/user-activity",
    "USERNAME",
    ["user-typing"],
    {}
  ]
}
```

To **stop** typing, send an empty array instead:
```json
{
  "params": ["ROOM_ID/user-activity", "USERNAME", [], {}]
}
```

### Receiving (subscribe)

```json
{
  "msg": "sub",
  "id": "...",
  "name": "stream-notify-room",
  "params": ["ROOM_ID/user-activity", false]
}
```

The `changed` event fields:
- `eventName`: `"ROOM_ID/user-activity"`
- `args[0]`: username (string)
- `args[1]`: array of activity types, e.g. `["user-typing"]` or `[]`

### Activity types (USER_ACTIVITIES constant)

| Constant | Value | Description |
|----------|-------|-------------|
| `USER_RECORDING` | `"user-recording"` | User is recording audio |
| `USER_TYPING` | `"user-typing"` | User is typing a message |
| `USER_UPLOADING` | `"user-uploading"` | User is uploading a file |
| `USER_PLAYING` | `"user-playing"` | User is playing audio |

### Timeout and renewal

- **TIMEOUT**: 15,000ms — if no renewal is received, the activity is cleared
- **RENEW**: 5,000ms — interval at which the web client resends the activity

This means typing indicators must be refreshed at least every 15 seconds to remain visible,
and the web client refreshes every 5 seconds.

### SDK mapping

The `sdk.publish('notify-room', args)` call translates to:
- DDPSDK transport ON: `DdpSdk.client.callAsync('stream-notify-room', ...args)`
- Legacy Meteor: `Meteor.call('stream-notify-room', ...args)`

The `sdk.stream('notify-room', [key], handler)` call translates to:
- `Meteor.connection.subscribe('stream-notify-room', key, ...)` 
- `DdpSdk.client.subscribe('stream-notify-room', key, ...)` 

The `stream-` prefix is added by the SDK to the stream name before the DDP call.

## What was wrong in the previous implementation

| Aspect | Old (wrong) | Correct |
|--------|-------------|---------|
| Event name | `{room_id}/typing` | `{room_id}/user-activity` |
| 3rd param | `true` / `false` (boolean) | `["user-typing"]` / `[]` (array) |
| 4th param | missing | `{}` (extras object for thread support) |

The old `typing` event may still be supported for backward compatibility on some servers,
but the modern canonical way is `user-activity` with the activity array.
