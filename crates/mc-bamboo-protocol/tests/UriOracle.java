import java.net.URISyntaxException;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import net.minecraft.util.Util;

/**
 * 用官方 {@code Util.parseAndValidateUntrustedUri} 判定一行一个的 Base64 URI。
 *
 * Judge one Base64 URI per line with official {@code Util.parseAndValidateUntrustedUri}.
 */
public class UriOracle {
    public static void main(String[] args) throws Exception {
        var in = new java.io.BufferedReader(
            new java.io.InputStreamReader(System.in, StandardCharsets.US_ASCII));
        String line;
        while ((line = in.readLine()) != null) {
            byte[] raw = line.isEmpty() ? new byte[0] : Base64.getDecoder().decode(line);
            String uri = new String(raw, StandardCharsets.UTF_8);
            boolean ok = true;
            try {
                Util.parseAndValidateUntrustedUri(uri);
            } catch (URISyntaxException ignored) {
                ok = false;
            }
            System.out.println(ok ? "1" : "0");
        }
    }
}
