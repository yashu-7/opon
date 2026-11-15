package io.github.yashu7.opon;

import android.app.NativeActivity;
import android.app.PendingIntent;
import android.content.Intent;
import android.content.IntentSender;
import com.google.android.gms.auth.api.identity.AuthorizationRequest;
import com.google.android.gms.auth.api.identity.AuthorizationResult;
import com.google.android.gms.auth.api.identity.Identity;
import com.google.android.gms.common.api.ApiException;
import com.google.android.gms.common.api.Scope;
import java.util.Collections;

public final class OponActivity extends NativeActivity {
    private static final String DRIVE_SCOPE = "https://www.googleapis.com/auth/drive.appdata";
    private int attempt;
    private int pendingCode = -1;
    private int pendingAttempt;
    private String driveResult;

    public synchronized void startDriveAuthorization(boolean interactive) {
        int current = ++attempt;
        driveResult = null;
        runOnUiThread(() -> authorize(current, interactive));
    }

    public synchronized String takeDriveAuthorizationResult() {
        String result = driveResult;
        driveResult = null;
        return result;
    }

    private void authorize(int current, boolean interactive) {
        AuthorizationRequest request = AuthorizationRequest.builder()
                .setRequestedScopes(Collections.singletonList(new Scope(DRIVE_SCOPE)))
                .build();
        Identity.getAuthorizationClient(this).authorize(request)
                .addOnSuccessListener(result -> {
                    if (!result.hasResolution()) {
                        complete(current, result);
                    } else if (!interactive) {
                        finishAttempt(current, "-expired");
                    } else {
                        showConsent(current, result.getPendingIntent());
                    }
                })
                .addOnFailureListener(error -> finishAttempt(current, "-unavailable"));
    }

    private synchronized void showConsent(int current, PendingIntent consent) {
        if (attempt != current) return;
        if (consent == null) {
            finishAttempt(current, "-unavailable");
            return;
        }
        pendingAttempt = current;
        pendingCode = 5000 + Math.floorMod(current, 50000);
        try {
            startIntentSenderForResult(consent.getIntentSender(), pendingCode, null, 0, 0, 0);
        } catch (IntentSender.SendIntentException error) {
            finishAttempt(current, "-unavailable");
        }
    }

    @Override
    protected synchronized void onActivityResult(int request, int resultCode, Intent data) {
        super.onActivityResult(request, resultCode, data);
        if (request != pendingCode) return;
        pendingCode = -1;
        if (resultCode != RESULT_OK || data == null) {
            finishAttempt(pendingAttempt, "-cancelled");
            return;
        }
        try {
            complete(pendingAttempt, Identity.getAuthorizationClient(this).getAuthorizationResultFromIntent(data));
        } catch (ApiException error) {
            finishAttempt(pendingAttempt, "-unavailable");
        }
    }

    private void complete(int current, AuthorizationResult result) {
        String token = result.getAccessToken();
        finishAttempt(current, token == null || token.isEmpty() ? "-unavailable" : "+" + token);
    }

    private synchronized void finishAttempt(int current, String result) {
        if (attempt == current) driveResult = result;
    }
}
