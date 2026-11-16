import 'dart:convert';

import 'package:http/http.dart' as http;

class SyncApi {
  SyncApi({required this.baseUrl, this.token, http.Client? client})
      : _client = client ?? http.Client();

  final String baseUrl;
  String? token;
  final http.Client _client;

  Map<String, String> get _headers => {
        'content-type': 'application/json',
        if (token != null) 'authorization': 'Bearer $token',
      };

  Future<Map<String, dynamic>> pair(
    String code,
    String deviceName,
  ) async {
    final response = await _client.post(
      Uri.parse('$baseUrl/api/v1/pair'),
      headers: _headers,
      body: jsonEncode({'code': code, 'device_name': deviceName}),
    );
    return _decode(response);
  }

  Future<Map<String, dynamic>> changes({
    required String deviceId,
    required int after,
  }) async {
    final response = await _client.get(
      Uri.parse('$baseUrl/api/v1/changes?after=$after'),
      headers: {..._headers, 'x-rtools-device-id': deviceId},
    );
    return _decode(response);
  }

  Future<Map<String, dynamic>> operations(
    String deviceId,
    Map<String, dynamic> batch,
  ) async {
    final response = await _client.post(
      Uri.parse('$baseUrl/api/v1/operations'),
      headers: {..._headers, 'x-rtools-device-id': deviceId},
      body: jsonEncode(batch),
    );
    return _decode(response);
  }

  Map<String, dynamic> _decode(http.Response response) {
    final body = response.body.isEmpty
        ? <String, dynamic>{}
        : jsonDecode(response.body) as Map<String, dynamic>;
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw SyncApiException(
        response.statusCode,
        body['code']?.toString() ?? 'HTTP_ERROR',
        body['message']?.toString() ?? response.body,
      );
    }
    return body;
  }
}

class SyncApiException implements Exception {
  SyncApiException(this.statusCode, this.code, this.message);

  final int statusCode;
  final String code;
  final String message;

  bool get tokenInvalid => statusCode == 401;

  @override
  String toString() => '$code: $message';
}
