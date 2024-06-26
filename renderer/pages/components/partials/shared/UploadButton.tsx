export type UploadButtonProps = {
  onChange: (event: React.ChangeEvent<HTMLInputElement>) => void;
  className?: string;
  accept: string;
};

const UploadButton = (props: UploadButtonProps) => {
  return (
    <div>
      <label htmlFor="fileInput" className={props.className}>
        Upload
      </label>
      <input
        id="fileInput"
        type="file"
        accept={props.accept}
        className="hidden"
        onChange={props.onChange}
      />
    </div>
  );
};

export default UploadButton